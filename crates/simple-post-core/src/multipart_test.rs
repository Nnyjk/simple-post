//! 装配的字节形状：boundary 一致、分片头、文件按路径现读、`Content-Length` 用的总长。
//!
//! 与 `multipart_wire_test.rs` 的分工：这里断言「装配出来的字节长这样」，那里断言
//! 「socket 上真的过去的是这个」。

use std::io::Write;

use futures_util::StreamExt;

use super::*;

/// 临时文件：路径唯一，落盘内容可控，drop 时删掉。
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn with(name: &str, bytes: &[u8]) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("simple-post-{}-{name}-{seq}", std::process::id()));
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(bytes))
            .expect("write the temp file");
        Self { path }
    }

    fn path_string(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn text_part(name: &str, value: &str) -> MultipartPart {
    MultipartPart {
        name: name.to_string(),
        value: value.to_string(),
        enabled: true,
        ..Default::default()
    }
}

fn file_part(name: &str, path: &str) -> MultipartPart {
    MultipartPart {
        name: name.to_string(),
        enabled: true,
        path: Some(path.to_string()),
        ..Default::default()
    }
}

/// 把一次装配的字节全读出来（走 `chunks`，与执行层同一条路）。
fn read_all(body: OutboundBody, chunk: usize) -> Result<Vec<u8>, std::io::Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async move {
            let mut stream = Box::pin(body.chunks(chunk));
            let mut out: Vec<u8> = Vec::new();
            while let Some(item) = stream.next().await {
                out.extend_from_slice(&item?);
            }
            Ok(out)
        })
}

#[test]
fn a_text_field_is_framed_by_the_boundary_from_the_content_type() {
    let assembled = assemble_with_boundary(&[text_part("title", "报告")], "XYZ").expect("assemble");

    assert_eq!(assembled.content_type, "multipart/form-data; boundary=XYZ");
    let bytes = read_all(assembled.body, CHUNK_BYTES).expect("read");
    let expected = concat!(
        "--XYZ\r\n",
        "Content-Disposition: form-data; name=\"title\"\r\n",
        "\r\n",
        "报告\r\n",
        "--XYZ--\r\n",
    );
    assert_eq!(String::from_utf8(bytes).expect("utf-8"), expected);
}

#[test]
fn a_disabled_part_stays_out_of_the_body() {
    let mut disabled = text_part("gone", "x");
    disabled.enabled = false;
    let assembled =
        assemble_with_boundary(&[text_part("kept", "y"), disabled], "XYZ").expect("assemble");

    let bytes = read_all(assembled.body, CHUNK_BYTES).expect("read");
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(text.contains("name=\"kept\""));
    assert!(
        !text.contains("name=\"gone\""),
        "a disabled part must not reach the body: {text}"
    );
}

#[test]
fn a_file_part_reads_its_path_and_defaults_name_and_type() {
    let file = TempFile::with("payload.bin", b"abc\0def");
    let parts = [file_part("upload", &file.path_string())];
    let assembled = assemble_with_boundary(&parts, "XYZ").expect("assemble");

    let body_len = assembled.body.len();
    let declared_len = assembled.body.len();
    assert_eq!(body_len, declared_len);

    let bytes = read_all(assembled.body, CHUNK_BYTES).expect("read");
    // 探测字节数与 `len()` 一致：`Content-Length` 就取自它。
    assert_eq!(bytes.len() as u64, body_len);

    let text = String::from_utf8_lossy(&bytes).to_string();
    assert!(
        text.contains(&format!(
            "Content-Disposition: form-data; name=\"upload\"; filename=\"{}\"",
            file.path.file_name().unwrap().to_string_lossy()
        )),
        "the file name must fall back to the path's last segment: {text}"
    );
    assert!(
        text.contains("Content-Type: application/octet-stream\r\n"),
        "a file part without an explicit type must fall back to the binary stream type: {text}"
    );
    assert!(
        bytes.windows(7).any(|w| w == b"abc\0def"),
        "the file's bytes must reach the body unchanged"
    );
}

#[test]
fn a_file_part_keeps_an_explicit_name_and_type() {
    let file = TempFile::with("payload.txt", b"hi");
    let mut part = file_part("upload", &file.path_string());
    part.filename = Some("报告.txt".to_string());
    part.content_type = Some("text/plain; charset=utf-8".to_string());

    let assembled = assemble_with_boundary(&[part], "XYZ").expect("assemble");
    let text =
        String::from_utf8(read_all(assembled.body, CHUNK_BYTES).expect("read")).expect("utf-8");

    assert!(text.contains("; filename=\"报告.txt\""), "{text}");
    assert!(
        text.contains("Content-Type: text/plain; charset=utf-8\r\n"),
        "{text}"
    );
}

#[test]
fn part_names_are_escaped_and_never_split_the_headers() {
    let assembled = assemble_with_boundary(&[text_part("a\"b\\c\r\nX-Injected: 1", "v")], "XYZ")
        .expect("assemble");
    let text =
        String::from_utf8(read_all(assembled.body, CHUNK_BYTES).expect("read")).expect("utf-8");

    assert!(
        text.contains(r#"name="a\"b\\cX-Injected: 1""#),
        "quotes and backslashes are escaped, CR/LF removed: {text}"
    );
    assert!(
        !text.contains("\r\nX-Injected:"),
        "a header parameter must not inject a header: {text}"
    );
}

#[test]
fn zero_parts_still_produce_a_well_formed_body() {
    let assembled = assemble_with_boundary(&[], "XYZ").expect("assemble");
    let bytes = read_all(assembled.body, CHUNK_BYTES).expect("read");
    assert_eq!(String::from_utf8(bytes).expect("utf-8"), "--XYZ--\r\n");
}

#[test]
fn chunks_are_independent_of_the_chunk_size() {
    let file = TempFile::with("big.bin", &vec![7u8; 200 * 1024]);
    let parts = [
        text_part("meta", "x"),
        file_part("payload", &file.path_string()),
        text_part("trailer", "y"),
    ];

    let whole = read_all(
        assemble_with_boundary(&parts, "XYZ")
            .expect("assemble")
            .body,
        usize::MAX,
    )
    .expect("read");

    for chunk in [1usize, 7, 4096, CHUNK_BYTES] {
        let piecewise = read_all(
            assemble_with_boundary(&parts, "XYZ")
                .expect("assemble")
                .body,
            chunk,
        )
        .expect("read");
        assert_eq!(
            piecewise.len(),
            whole.len(),
            "chunk size {chunk} changed the total length"
        );
        assert_eq!(piecewise, whole, "chunk size {chunk} changed the bytes");
    }
}

#[test]
fn a_file_that_shrank_after_assembly_fails_instead_of_sending_less() {
    let file = TempFile::with("shrink.bin", b"0123456789");
    let assembled =
        assemble_with_boundary(&[file_part("p", &file.path_string())], "XYZ").expect("assemble");
    std::fs::write(&file.path, b"012").expect("truncate");

    let err = read_all(assembled.body, 4).expect_err("must fail");
    assert_eq!(
        err.kind(),
        std::io::ErrorKind::UnexpectedEof,
        "a shorter file must be an error, not a shorter body"
    );
}

#[test]
fn a_file_that_grew_after_assembly_fails_instead_of_sending_a_prefix() {
    let file = TempFile::with("grow.bin", b"0123456789");
    let assembled =
        assemble_with_boundary(&[file_part("p", &file.path_string())], "XYZ").expect("assemble");
    std::fs::write(&file.path, b"0123456789ABCDEF").expect("append");

    let err = read_all(assembled.body, 4).expect_err("must fail");
    assert_eq!(
        err.kind(),
        std::io::ErrorKind::InvalidData,
        "a longer file must be an error, not a silently truncated body"
    );
}

#[test]
fn a_missing_file_is_reported_at_assembly_time() {
    let err = assemble_with_boundary(&[file_part("p", "definitely/not/here.bin")], "XYZ")
        .expect_err("assemble must fail");
    assert!(err.contains("definitely/not/here.bin"), "{err}");
}

#[test]
fn a_directory_is_not_a_file() {
    let dir = std::env::temp_dir();
    let err = assemble_with_boundary(&[file_part("p", &dir.to_string_lossy())], "XYZ")
        .expect_err("assemble must fail");
    assert!(err.contains("not a regular file"), "{err}");
}

#[test]
fn a_binary_body_is_one_file_and_has_no_inline_bytes() {
    let file = TempFile::with("body.bin", &[0u8, 1, 2, 3]);
    let body = OutboundBody::from_file(&BodyFile {
        path: file.path_string(),
        ..Default::default()
    })
    .expect("from_file");

    assert_eq!(body.len(), 4);
    assert!(!body.is_empty());
    assert!(
        body.as_inline().is_none(),
        "a file body is never inline: the bytes are read on send"
    );
    assert_eq!(
        read_all(body, CHUNK_BYTES).expect("read"),
        vec![0u8, 1, 2, 3]
    );
}

#[test]
fn a_binary_body_without_a_path_is_an_error() {
    let err = OutboundBody::from_file(&BodyFile::default()).expect_err("must fail");
    assert!(err.contains("no file path"), "{err}");
}

#[test]
fn an_inline_body_hands_its_bytes_back_untouched() {
    let body = OutboundBody::inline(b"{\"a\":1}".to_vec());
    assert_eq!(body.len(), 7);
    assert_eq!(body.as_inline(), Some(&b"{\"a\":1}"[..]));
    assert!(OutboundBody::inline(Vec::new()).is_empty());
}

/// 一次 Web 形态的上传：`request` 里声明了长度的分片按顺序从上传来。
fn upload_part(name: &str, size: u64) -> MultipartPart {
    MultipartPart {
        name: name.to_string(),
        enabled: true,
        // 浏览器知道选中的文件名，它随请求一起上来；缺了它分片只剩名字。
        filename: Some(format!("{name}.bin")),
        size: Some(size),
        ..Default::default()
    }
}

/// 一个按顺序吐帧的上传端：`Part` → 逐块 `Data` → `End`。
fn upload_channel(
    chunks: &'static [&'static [u8]],
) -> (UploadReceiver, tokio::task::JoinHandle<()>) {
    let (tx, rx) = tokio::sync::mpsc::channel::<UploadFrame>(8);
    let pump = tokio::spawn(async move {
        let _ = tx.send(UploadFrame::Part).await;
        for chunk in chunks {
            let _ = tx.send(UploadFrame::Data(Bytes::from_static(chunk))).await;
        }
        let _ = tx.send(UploadFrame::End).await;
    });
    (rx, pump)
}

/// 把 `chunks` 读到底：成功给整段字节，失败给错误。
async fn drain(body: OutboundBody) -> Result<Vec<u8>, std::io::Error> {
    let mut stream = Box::pin(body.chunks(4));
    let mut out: Vec<u8> = Vec::new();
    while let Some(item) = stream.next().await {
        out.extend_from_slice(&item?);
    }
    Ok(out)
}

#[tokio::test]
async fn an_uploaded_file_is_framed_like_a_file_from_disk() {
    let (rx, pump) = upload_channel(&[b"012", b"3456"]);
    let assembled = assemble_with_uploads(&[upload_part("f", 7)], rx).expect("assemble");
    let declared = assembled.body.len();
    let bytes = drain(assembled.body).await.expect("read");
    pump.await.expect("pump");

    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(
        text.contains("Content-Disposition: form-data; name=\"f\"; filename=\"f.bin\""),
        "{text}"
    );
    assert!(
        text.contains("Content-Type: application/octet-stream"),
        "{text}"
    );
    assert!(text.contains("\r\n0123456\r\n"), "{text}");
    assert_eq!(
        declared as usize,
        text.len(),
        "the declared size must be what actually goes out"
    );
}

#[tokio::test]
async fn an_upload_shorter_than_its_declared_size_fails() {
    let (rx, pump) = upload_channel(&[b"012"]);
    let assembled = assemble_with_uploads(&[upload_part("f", 7)], rx).expect("assemble");
    let err = drain(assembled.body).await.expect_err("must fail");
    pump.await.expect("pump");
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn an_upload_longer_than_its_declared_size_fails() {
    let (rx, pump) = upload_channel(&[b"0123456789"]);
    let assembled = assemble_with_uploads(&[upload_part("f", 4)], rx).expect("assemble");
    let err = drain(assembled.body).await.expect_err("must fail");
    pump.await.expect("pump");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn two_uploaded_files_take_their_bytes_in_declaration_order() {
    let (tx, rx) = tokio::sync::mpsc::channel::<UploadFrame>(8);
    let pump = tokio::spawn(async move {
        for (chunk, size) in [(&b"aaa"[..], 3u64), (&b"bbb"[..], 3u64)] {
            let _ = tx.send(UploadFrame::Part).await;
            let _ = tx
                .send(UploadFrame::Data(Bytes::copy_from_slice(chunk)))
                .await;
            let _ = tx.send(UploadFrame::End).await;
            assert_eq!(chunk.len() as u64, size);
        }
    });
    let assembled =
        assemble_with_uploads(&[upload_part("a", 3), upload_part("b", 3)], rx).expect("assemble");
    let text = String::from_utf8(drain(assembled.body).await.expect("read")).expect("utf-8");
    pump.await.expect("pump");
    assert!(
        text.find("name=\"a\"").expect("a") < text.find("name=\"b\"").expect("b"),
        "{text}"
    );
    assert!(text.contains("\r\naaa\r\n"), "{text}");
    assert!(text.contains("\r\nbbb\r\n"), "{text}");
}

#[test]
fn uploaded_bytes_without_a_file_part_are_rejected() {
    // 信封里没有任何声明长度的分片，通道里却有字节：这些字节没有分片接收，
    // 装出来的请求体会把它们静默丢掉。
    let (tx, rx) = tokio::sync::mpsc::channel::<UploadFrame>(8);
    drop(tx);
    let parts = [MultipartPart {
        name: "t".into(),
        value: "v".into(),
        enabled: true,
        ..Default::default()
    }];
    let err = assemble_with_uploads(&parts, rx).expect_err("must fail");
    assert!(err.contains("declares none"), "{err}");
}

#[test]
fn a_file_part_with_a_declared_size_needs_uploaded_bytes() {
    let err = assemble(&[upload_part("f", 4)]).expect_err("assemble must fail");
    assert!(err.contains("no uploaded bytes"), "{err}");
}

#[test]
fn set_content_type_replaces_in_place_and_appends_when_absent() {
    let mut headers = vec![
        ("Accept".to_string(), "*/*".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
    ];
    set_content_type(&mut headers, "multipart/form-data; boundary=A");
    assert_eq!(
        headers,
        vec![
            ("Accept".to_string(), "*/*".to_string()),
            (
                "content-type".to_string(),
                "multipart/form-data; boundary=A".to_string()
            ),
        ],
        "an existing Content-Type is overwritten in place, keeping the user's name"
    );

    let mut headers = vec![("Accept".to_string(), "*/*".to_string())];
    set_content_type(&mut headers, "multipart/form-data; boundary=B");
    assert_eq!(
        headers[1],
        (
            "Content-Type".to_string(),
            "multipart/form-data; boundary=B".to_string()
        )
    );
}

#[test]
fn duplicate_content_types_collapse_to_the_one_with_the_boundary() {
    let mut headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Accept".to_string(), "*/*".to_string()),
        ("content-type".to_string(), "text/plain".to_string()),
    ];
    set_content_type(&mut headers, "multipart/form-data; boundary=A");
    assert_eq!(
        headers,
        vec![
            (
                "Content-Type".to_string(),
                "multipart/form-data; boundary=A".to_string()
            ),
            ("Accept".to_string(), "*/*".to_string()),
        ],
        "only one Content-Type survives, and it carries the boundary"
    );
}

#[test]
fn a_fresh_boundary_is_unique_per_call_and_fits_the_rfc_length_cap() {
    let a = new_boundary();
    let b = new_boundary();
    assert_ne!(a, b);
    assert!(a.len() <= 70, "RFC 2046 caps the boundary at 70: {a}");
}
