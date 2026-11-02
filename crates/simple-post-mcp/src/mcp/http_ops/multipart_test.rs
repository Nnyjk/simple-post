use super::*;

fn parts(json: &str) -> Vec<MultipartPart> {
    simple_post_core::body::parse_multipart_parts(json)
}

/// body 的字节 → 文本。装配出来的字节在纯文本分片时一定是合法 UTF-8。
fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("纯文本分片装配出的字节是合法 UTF-8")
}

#[test]
fn boundary_in_body_and_content_type_is_the_same_string() {
    let b = new_boundary();
    let ct = content_type(&b);
    let body = text_of(build_body(&parts(r#"[{"name":"f","value":"v"}]"#), &b));

    // Content-Type 里声明的 boundary 必须逐字出现在 body 里，否则请求看起来成功而
    // 服务端解析失败
    let declared = ct.strip_prefix("multipart/form-data; boundary=").unwrap();
    assert_eq!(declared, b);
    assert!(body.contains(&format!("--{b}\r\n")));
    assert!(body.contains(&format!("--{b}--\r\n")));
}

#[test]
fn boundary_differs_per_call() {
    assert_ne!(new_boundary(), new_boundary());
}

#[test]
fn boundary_is_a_legal_rfc2046_token() {
    let b = new_boundary();
    assert!(b.len() <= 70, "RFC 2046 caps the boundary at 70 chars");
    assert!(b.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'));
}

#[test]
fn builds_content_disposition_and_crlf_framed_parts() {
    let b = "BOUND";
    let body = text_of(build_body(
        &parts(r#"[{"name":"a","value":"1"},{"name":"b","value":"2"}]"#),
        b,
    ));
    assert_eq!(
        body,
        "--BOUND\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n\
         --BOUND\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\n2\r\n--BOUND--\r\n"
    );
}

#[test]
fn filename_makes_it_a_file_part_and_defaults_its_content_type() {
    let body = text_of(build_body(
        &parts(r#"[{"name":"f","value":"data","filename":"a.txt"}]"#),
        "B",
    ));
    assert!(body.contains("Content-Disposition: form-data; name=\"f\"; filename=\"a.txt\""));
    assert!(body.contains("Content-Type: application/octet-stream"));
}

#[test]
fn explicit_content_type_wins_over_the_file_default() {
    let body = text_of(build_body(
        &parts(r#"[{"name":"f","value":"d","filename":"a.png","contentType":"image/png"}]"#),
        "B",
    ));
    assert!(body.contains("Content-Type: image/png"));
    assert!(!body.contains("application/octet-stream"));
}

#[test]
fn plain_field_gets_no_content_type() {
    let body = text_of(build_body(&parts(r#"[{"name":"a","value":"1"}]"#), "B"));
    assert!(!body.contains("Content-Type:"));
}

#[test]
fn header_params_escape_quotes_and_strip_crlf() {
    // CR/LF 留着就是往请求头里注入一个新头
    let body = text_of(build_body(
        &parts(r#"[{"name":"a\"b\r\nX-Evil: 1","value":"v","filename":"c\\d.txt"}]"#),
        "B",
    ));
    assert!(body.contains("name=\"a\\\"bX-Evil: 1\""));
    assert!(body.contains("filename=\"c\\\\d.txt\""));
    assert!(!body.contains("X-Evil: 1\r\n"));
}

#[test]
fn zero_parts_still_produces_a_well_formed_body() {
    assert_eq!(build_body(&[], "B"), b"--B--\r\n");
}

#[test]
fn malformed_json_yields_zero_parts() {
    assert!(parts("not json").is_empty());
    assert!(parts("").is_empty());
    assert!(parts("{}").is_empty());
}

#[test]
fn a_wrongly_typed_field_discards_the_whole_list() {
    // 与 TS 侧同形：逐字段回退会让同一个畸形输入在两侧得到不同的分片
    assert!(parts(r#"[{"name":"a","value":10}]"#).is_empty());
    assert!(parts(r#"[{"name":1,"value":"x"}]"#).is_empty());
    assert!(parts(r#"["a"]"#).is_empty());
}

#[test]
fn optional_fields_default_and_empty_strings_are_kept() {
    let parsed = parts(r#"[{"name":"a"},{"name":"b","value":"","filename":""}]"#);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].name, "a");
    assert_eq!(parsed[0].value, "");
    assert!(parsed[0].filename.is_none());
    assert_eq!(parsed[1].value, "");

    // 空 filename 不构成文件分片，故不回落 Content-Type
    let body = text_of(build_body(&parsed, "B"));
    assert!(body.contains("name=\"b\""));
    assert!(!body.contains("filename="));
    assert!(!body.contains("Content-Type:"));
}

#[test]
fn camel_case_content_type_is_the_wire_field() {
    let parsed = parts(r#"[{"name":"f","value":"d","contentType":"image/png"}]"#);
    assert_eq!(parsed[0].content_type.as_deref(), Some("image/png"));
}

#[test]
fn an_explicit_null_optional_field_is_the_same_as_omitting_it() {
    // serde 的 `Option` 把 JSON `null` 读成 `None`——与「不写这个键」同义。
    // TS 侧必须同形：否则 agent 写 `"filename": null` 时，MCP 发一个分片、
    // UI 发零分片，同一个 body 成了两份。
    let parsed = parts(r#"[{"name":"f","value":"v","filename":null,"contentType":null}]"#);
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].filename.is_none());
    assert!(parsed[0].content_type.is_none());

    let body = text_of(build_body(&parsed, "B"));
    assert!(body.contains("name=\"f\""));
    assert!(!body.contains("filename="));
    assert!(!body.contains("Content-Type:"));
}

// ============================================================
// 文件分片：内容是字节
// ============================================================

#[test]
fn a_file_part_carries_its_bytes() {
    // "你好" 的 UTF-8 字节是 e4 bd a0 e5 a5 bd，base64 之后是 5L2g5aW9。
    let body = build_body(
        &parts(r#"[{"name":"f","filename":"a.txt","base64":"5L2g5aW9"}]"#),
        "B",
    );
    assert!(body.windows(6).any(|w| w == "你好".as_bytes()));
}

#[test]
fn a_file_part_with_undecodable_base64_has_no_content() {
    // 解不开的 base64 给空内容：发一个内容被改写过的文件比发一个空文件更容易发现。
    let body = text_of(build_body(
        &parts(r#"[{"name":"f","filename":"a.txt","base64":"!!!"}]"#),
        "B",
    ));
    assert!(body.contains("name=\"f\""));
    assert!(body.ends_with("\r\n\r\n--B--\r\n"));
}

#[test]
fn a_binary_body_comes_out_as_base64() {
    // 字节不是合法 UTF-8，故产物走 base64 形态。
    let mut headers: Vec<(String, String)> = vec![];
    let assembled = apply(
        &mut headers,
        r#"[{"name":"f","filename":"a.bin","base64":"/w=="}]"#,
    );
    assert!(assembled.text.is_none());
    let base64 = assembled.base64.expect("二进制形态");
    // base64 里看不到分隔符本身，得解开再断言。
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(base64.as_bytes())
        .expect("base64 形态必须能解开");
    assert!(
        decoded.starts_with(b"--"),
        "解开的字节必须还是那个 multipart body: {decoded:?}"
    );
    assert!(
        decoded.ends_with(b"\r\n"),
        "解开的字节必须以 CRLF 收尾: {decoded:?}"
    );
}

// ============================================================
// apply：Content-Type 覆盖 + body 装配
// ============================================================

#[test]
fn apply_overrides_a_user_set_content_type_because_the_body_is_ours() {
    // 其余模式的 body 是用户手写的，用户那条 Content-Type 有机会描述它；multipart
    // 的 body 是我们用 boundary 框出来的，用户那条值描述不了它
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    let assembled = apply(&mut headers, r#"[{"name":"a","value":"1"}]"#);

    assert_eq!(headers.len(), 1, "the header is replaced, not duplicated");
    assert_eq!(
        headers[0].0, "content-type",
        "the user's header name casing is kept"
    );
    let b = headers[0]
        .1
        .strip_prefix("multipart/form-data; boundary=")
        .expect("a multipart Content-Type");
    let body = assembled.text.expect("纯文本分片装配出的字节是合法 UTF-8");
    assert!(body.starts_with(&format!("--{b}\r\n")));
    assert!(body.ends_with(&format!("--{b}--\r\n")));
}

#[test]
fn apply_appends_a_content_type_when_the_user_set_none() {
    let mut headers: Vec<(String, String)> =
        vec![("accept".to_string(), "application/json".into())];
    let assembled = apply(&mut headers, r#"[{"name":"a","value":"1"}]"#);

    assert_eq!(headers.len(), 2);
    assert_eq!(headers[1].0, "Content-Type");
    let b = headers[1]
        .1
        .strip_prefix("multipart/form-data; boundary=")
        .expect("a multipart Content-Type");
    let body = assembled.text.expect("纯文本分片装配出的字节是合法 UTF-8");
    assert!(body.starts_with(&format!("--{b}\r\n")));
}

#[test]
fn apply_matches_the_wire_mode_by_a_case_insensitive_header_name() {
    let mut headers = vec![("CONTENT-TYPE".to_string(), "text/plain".to_string())];
    let _ = apply(&mut headers, "[]");
    assert_eq!(headers.len(), 1);
    assert!(headers[0].1.starts_with("multipart/form-data; boundary="));
}

/// 跨语言 golden：这一份期望值是 `src/lib/multipart.ts` 的实际产物，TS 侧
/// `http-build-multipart.test.ts` 断言同一份字节。
///
/// 两套实现是各自写一遍的规则（边界字符集、转义、缺省 Content-Type、畸形输入的
/// 处理），而它们的产物一旦不同，两侧会各自信任自己那份，服务端却只认其中一个。
/// 同一个 golden 被两侧断言，任何一边改动都会红，不必靠人记得同步。
#[test]
fn the_body_matches_the_typescript_implementation_byte_for_byte() {
    const B: &str = "----SimplePostFormBoundaryFIXED";
    let content = concat!(
        r#"[{"name":"title","value":"报告"},"#,
        r#"{"name":"file","value":"binary-ish","filename":"a.txt"},"#,
        r#"{"name":"note","value":"","contentType":"text/plain"},"#,
        r#"{"name":"we\"ird\\name\r\ninjected","value":"v"},"#,
        r#"{"name":"emptyFile","value":"x","filename":"b.bin"},"#,
        r#"{"name":"nullFields","value":"v","filename":null,"contentType":null}]"#
    );

    let expected = [
        format!("--{B}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\n报告"),
        format!("--{B}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\nContent-Type: application/octet-stream\r\n\r\nbinary-ish"),
        format!("--{B}\r\nContent-Disposition: form-data; name=\"note\"\r\nContent-Type: text/plain\r\n\r\n"),
        // 分片名里的引号与反斜杠被转义，CR/LF 整个被剥掉——留着就是往头里注入一个新头。
        format!("--{B}\r\nContent-Disposition: form-data; name=\"we\\\"ird\\\\nameinjected\"\r\n\r\nv"),
        format!("--{B}\r\nContent-Disposition: form-data; name=\"emptyFile\"; filename=\"b.bin\"\r\nContent-Type: application/octet-stream\r\n\r\nx"),
        // 显式 null 是缺省：分片留着，那两个字段没有值。TS 侧是同一个语义。
        format!("--{B}\r\nContent-Disposition: form-data; name=\"nullFields\"\r\n\r\nv"),
    ]
    .join("\r\n")
        + &format!("\r\n--{B}--\r\n");

    assert_eq!(build_body(&parts(content), B), expected.as_bytes());
}
