//! `multipart/form-data` 的装配 —— Content-Type 的 boundary 与 body 里的 boundary
//! **必须是同一个字符串**，否则服务端解析失败而请求看起来成功。
//!
//! **装配只有这一处实现。** 分片结构存在 [`RequestBody::content`] 里，是一个分片数组的
//! JSON 文本；文件分片只记路径，字节由这里在发请求时按路径现读并流式写出。故库里、历史
//! 快照里与线上 JSON 里都没有文件内容，大文件也不会被整段读进内存。
//!
//! 产物是 [`OutboundBody`]：一串「整段字节」与「文件」的分段，附带**精确**的总字节数。
//! 执行层拿它流式发出，并据此补 `Content-Length`（流式 body 没有长度提示，缺了它服务端会
//! 把请求读成 chunked）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures_util::Stream;

use crate::domain::{BodyFile, MultipartPart};

/// 文件分片没写 `contentType` 时的缺省值，与浏览器发 FormData 一致。
const DEFAULT_FILE_CONTENT_TYPE: &str = "application/octet-stream";

/// 一次读文件的字节数。64 KiB 是进度粒度与系统调用次数之间的折中。
pub const CHUNK_BYTES: usize = 64 * 1024;

/// 现生成一个 boundary。RFC 2046 限长 70 且不允许出现在分片内容里，所以用
/// 固定前缀 + 哈希指纹。
///
/// **每次请求现生成**：桌面与 `serve-web` 各生成各的，只要各自的 body 与 Content-Type
/// 用的是同一个值即可，不需要跨路径一致。
///
/// 本 crate 没有 `rand` 依赖，故用纳秒时间戳 + 进程号 + 计数器过 SipHash。边界需要的是
/// 「本次请求内不与内容撞」，不是密码学随机。
pub fn new_boundary() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut hasher = DefaultHasher::new();
    (nanos, std::process::id(), seq).hash(&mut hasher);
    format!("----SimplePostFormBoundary{:016x}", hasher.finish())
}

/// 带 boundary 的 Content-Type。
pub fn content_type(boundary: &str) -> String {
    format!("multipart/form-data; boundary={boundary}")
}

/// 把请求头里那条 Content-Type 的值换成 `value`；没有这一条就追加到末尾。
///
/// **multipart 是「用户值优先」的唯一例外。** 其余模式的 body 是用户手写的，用户自设的
/// Content-Type 有机会描述它；multipart 的 body 是这里用 boundary 框出来的，boundary 是
/// 生成物的一部分，用户那条值在结构上描述不了它 —— 留着就是「存得对、发得错」。
///
/// 重名的 Content-Type 只留第一条（改成 `value`），其余丢弃：请求头是允许重名的列表，
/// 而两条不同的 Content-Type 里只有一条带 boundary，保留了第二条等于让服务端可能解析失败。
pub fn set_content_type(headers: &mut Vec<(String, String)>, value: &str) {
    let mut seen = false;
    headers.retain_mut(|(name, current)| {
        if !name.eq_ignore_ascii_case("content-type") {
            return true;
        }
        if seen {
            return false;
        }
        seen = true;
        *current = value.to_string();
        true
    });
    if !seen {
        headers.push(("Content-Type".to_string(), value.to_string()));
    }
}

/// 头参数里的引号与反斜杠要转义，而 CR/LF 必须整个去掉——留着就是往响应头里
/// 注入一个新头，服务端看到的分片名与用户填的不是一回事。
fn escape_header_param(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .collect::<String>()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// 分片自己的 Content-Type：写了就用，没写且是文件分片则回落到二进制流类型。
fn part_content_type(part: &MultipartPart) -> Option<&str> {
    if let Some(ct) = part.content_type.as_deref().filter(|s| !s.is_empty()) {
        return Some(ct);
    }
    if is_file_part(part) {
        return Some(DEFAULT_FILE_CONTENT_TYPE);
    }
    None
}

/// 这一分片是不是文件分片。两种来源：本机路径（桌面、MCP），或这次请求上传的字节
/// （Web 形态，靠声明的 `size` 认出来）。两种都不是就是文本分片。
pub fn is_file_part(part: &MultipartPart) -> bool {
    part.path.as_deref().is_some_and(|p| !p.is_empty()) || part.size.is_some()
}

/// `Content-Disposition` 里的文件名：写了就用，没写就取路径的最后一段。
fn file_name(part: &MultipartPart) -> Option<String> {
    if let Some(name) = part.filename.as_deref().filter(|s| !s.is_empty()) {
        return Some(name.to_string());
    }
    let path = part.path.as_deref()?;
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// 一次请求里由客户端上传的文件字节，一帧一帧送来。
///
/// Web 形态下浏览器选的文件没有服务端可读的路径：字节随发送请求一起上来，服务端边收边
/// 转发、不落盘。帧按请求体里文件分片的先后顺序排列，故消费端按同一顺序取。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadFrame {
    /// 下一个文件分片的字节从这里开始。
    Part,
    /// 一段字节。
    Data(Bytes),
    /// 当前文件分片结束。
    End,
}

/// 上传字节的通道。生产者（HTTP 入口）按分片顺序写入，消费者（发送执行器）按同一顺序读。
pub type UploadReceiver = tokio::sync::mpsc::Receiver<UploadFrame>;

/// 请求体的一段：整段字节、一个按路径现读的文件，或一段由这次请求上传的字节。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Bytes(Vec<u8>),
    File { path: PathBuf, len: u64 },
    Upload { len: u64 },
}

/// 一次请求要发出去的字节。总长在装配时算好（分片头 + 文本 + 文件元数据），执行层据它
/// 补 `Content-Length` 与进度分母。
///
/// 上传段的总长来自调用方声明的 `size`：通道里只有字节，没有长度可量，故声明值是唯一的
/// 分母来源；实收字节与它不符时按错误中止，不静默补发或少发。
#[derive(Debug, Default)]
pub struct OutboundBody {
    segments: Vec<Segment>,
    len: u64,
    uploads: Option<UploadReceiver>,
}

impl OutboundBody {
    /// 一整段已经在内存里的字节（`json` / `form` / `multipart` 以外的模式）。
    pub fn inline(bytes: Vec<u8>) -> Self {
        Self {
            len: bytes.len() as u64,
            segments: if bytes.is_empty() {
                Vec::new()
            } else {
                vec![Segment::Bytes(bytes)]
            },
            uploads: None,
        }
    }

    /// 一个文件就是整个请求体（`binary` 模式）。
    pub fn from_file(file: &BodyFile) -> Result<Self, String> {
        if file.path.trim().is_empty() {
            return Err("the binary request body has no file path".to_string());
        }
        let len = file_len(&file.path)?;
        Ok(Self {
            segments: vec![Segment::File {
                path: PathBuf::from(&file.path),
                len,
            }],
            len,
            uploads: None,
        })
    }

    /// `binary` 模式，文件由这次请求上传：长度取声明值，字节从通道里来。
    pub fn from_upload(len: u64, uploads: UploadReceiver) -> Self {
        Self {
            segments: vec![Segment::Upload { len }],
            len,
            uploads: Some(uploads),
        }
    }

    /// 请求体有多少字节。执行层用它补 `Content-Length`，也用它当进度分母。
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 整段字节都在内存里时直接给出——省掉一次流式包装。
    pub fn as_inline(&self) -> Option<&[u8]> {
        match self.segments.as_slice() {
            [Segment::Bytes(bytes)] => Some(bytes),
            _ => None,
        }
    }

    /// 逐块取字节：内存段按 `chunk` 切开，文件段按 `chunk` 读，上传段按帧收。
    ///
    /// 文件在装配时只读了元数据；读盘发生在这里，故文件大小在装配之后被改动时，
    /// 读到的字节数与 `Content-Length` 对不上——两种情况都以错误报出，而不是悄悄少发
    /// 或者多发几个字节。上传段同理：实收字节数与调用方声明的 `size` 不符就中止。
    pub fn chunks(self, chunk: usize) -> impl Stream<Item = Result<Vec<u8>, std::io::Error>> {
        struct State {
            segments: std::vec::IntoIter<Segment>,
            pending_bytes: Option<(Vec<u8>, usize)>,
            pending_file: Option<(tokio::fs::File, u64)>,
            /// 当前上传分片：(还剩多少字节，是否已经收到 `Part` 起始标记)。
            pending_upload: Option<(u64, bool)>,
            uploads: Option<UploadReceiver>,
            chunk: usize,
        }

        let state = State {
            segments: self.segments.into_iter(),
            pending_bytes: None,
            pending_file: None,
            pending_upload: None,
            uploads: self.uploads,
            chunk: chunk.max(1),
        };

        futures_util::stream::unfold(state, |mut st| async move {
            loop {
                if let Some((bytes, offset)) = st.pending_bytes.take() {
                    if offset < bytes.len() {
                        let end = (offset + st.chunk).min(bytes.len());
                        let piece = bytes[offset..end].to_vec();
                        st.pending_bytes = Some((bytes, end));
                        return Some((Ok(piece), st));
                    }
                }

                if let Some((mut file, left)) = st.pending_file.take() {
                    if left > 0 {
                        let want = (st.chunk as u64).min(left) as usize;
                        let mut buf = vec![0u8; want];
                        match tokio::io::AsyncReadExt::read(&mut file, &mut buf).await {
                            Ok(0) => {
                                let err = std::io::Error::new(
                                    std::io::ErrorKind::UnexpectedEof,
                                    "the file shrank after the request was assembled",
                                );
                                return Some((Err(err), st));
                            }
                            Ok(n) => {
                                buf.truncate(n);
                                st.pending_file = Some((file, left - n as u64));
                                return Some((Ok(buf), st));
                            }
                            Err(e) => return Some((Err(e), st)),
                        }
                    } else {
                        // 装配时锁定的字节已经发完：再读一个字节，读到了说明文件在装配之后
                        // 长过。静默少发一半文件与少发整个文件一样是数据完整性问题，故报错。
                        let mut probe = [0u8; 1];
                        match tokio::io::AsyncReadExt::read(&mut file, &mut probe).await {
                            Ok(0) => {}
                            Ok(_) => {
                                let err = std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "the file grew after the request was assembled",
                                );
                                return Some((Err(err), st));
                            }
                            Err(e) => return Some((Err(e), st)),
                        }
                    }
                }

                if let Some((left, started)) = st.pending_upload {
                    let frame = match st.uploads.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => None,
                    };
                    match frame {
                        Some(UploadFrame::Part) if !started => {
                            st.pending_upload = Some((left, true));
                        }
                        Some(UploadFrame::Data(bytes)) if started => {
                            if bytes.len() as u64 > left {
                                let err = std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "the uploaded file is longer than the declared size",
                                );
                                return Some((Err(err), st));
                            }
                            st.pending_upload = Some((left - bytes.len() as u64, true));
                            if !bytes.is_empty() {
                                return Some((Ok(bytes.to_vec()), st));
                            }
                        }
                        Some(UploadFrame::End) if started && left == 0 => {
                            st.pending_upload = None;
                        }
                        Some(UploadFrame::End) => {
                            let err = std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "the uploaded file is shorter than the declared size",
                            );
                            return Some((Err(err), st));
                        }
                        Some(UploadFrame::Data(_)) | Some(UploadFrame::Part) => {
                            let err = std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "the upload stream is out of frame order",
                            );
                            return Some((Err(err), st));
                        }
                        None => {
                            let err = std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "the upload stream ended before every file part arrived",
                            );
                            return Some((Err(err), st));
                        }
                    }
                    continue;
                }

                match st.segments.next() {
                    Some(Segment::Bytes(bytes)) => st.pending_bytes = Some((bytes, 0)),
                    Some(Segment::File { path, len }) => match tokio::fs::File::open(&path).await {
                        Ok(file) => st.pending_file = Some((file, len)),
                        Err(e) => {
                            let err = std::io::Error::new(
                                e.kind(),
                                format!("unable to open `{}`: {e}", path.display()),
                            );
                            return Some((Err(err), st));
                        }
                    },
                    Some(Segment::Upload { len }) => st.pending_upload = Some((len, false)),
                    None => return None,
                }
            }
        })
    }
}

/// 装配好的一次 multipart 请求体：带 boundary 的 Content-Type 与按路径现读的字节。
#[derive(Debug)]
pub struct AssembledMultipart {
    pub content_type: String,
    pub body: OutboundBody,
}

/// 分片数组 + 现生成的 boundary → 装配好的请求体。文件分片只按本机路径取字节。
pub fn assemble(parts: &[MultipartPart]) -> Result<AssembledMultipart, String> {
    let boundary = new_boundary();
    assemble_inner(parts, &boundary, None)
}

/// 分片数组 + 这次请求上传的字节 → 装配好的请求体。
///
/// Web 形态用这一条：没有路径的文件分片（带声明的 `size`）从 `uploads` 里收字节，一个分片
/// 一段帧。声明长度是唯一的分母来源，故没有 `size` 的文件分片在这里被拒绝。
pub fn assemble_with_uploads(
    parts: &[MultipartPart],
    uploads: UploadReceiver,
) -> Result<AssembledMultipart, String> {
    let boundary = new_boundary();
    assemble_inner(parts, &boundary, Some(uploads))
}

/// 分片数组 + 指定 boundary → 装配好的请求体。boundary 由调用方给时用于测试。
///
/// 零个分片得到 `--<boundary>--\r\n`，仍是格式合法的 multipart。
pub fn assemble_with_boundary(
    parts: &[MultipartPart],
    boundary: &str,
) -> Result<AssembledMultipart, String> {
    assemble_inner(parts, boundary, None)
}

fn assemble_inner(
    parts: &[MultipartPart],
    boundary: &str,
    uploads: Option<UploadReceiver>,
) -> Result<AssembledMultipart, String> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut len: u64 = 0;
    // 通道里有没有被认领的分片。没有等于说客户端送来的字节没有任何分片接收，
    // 那是信封与请求体对不上，必须报错而不是静默丢掉。
    let mut claimed_uploads = uploads.is_none();

    fn push_bytes(segments: &mut Vec<Segment>, len: &mut u64, bytes: Vec<u8>) {
        *len += bytes.len() as u64;
        segments.push(Segment::Bytes(bytes));
    }

    for part in parts {
        // 停用的分片不进 body：它还留在表格里，用户改主意时勾回来即可。
        if !part.enabled {
            continue;
        }

        let mut head: Vec<u8> = Vec::new();
        head.extend_from_slice(b"--");
        head.extend_from_slice(boundary.as_bytes());
        head.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
        head.extend_from_slice(escape_header_param(&part.name).as_bytes());
        head.push(b'"');
        if let Some(filename) = file_name(part) {
            head.extend_from_slice(b"; filename=\"");
            head.extend_from_slice(escape_header_param(&filename).as_bytes());
            head.push(b'"');
        }
        head.extend_from_slice(b"\r\n");
        if let Some(ct) = part_content_type(part) {
            head.extend_from_slice(b"Content-Type: ");
            head.extend_from_slice(ct.as_bytes());
            head.extend_from_slice(b"\r\n");
        }
        head.extend_from_slice(b"\r\n");
        push_bytes(&mut segments, &mut len, head);

        match part.path.as_deref().filter(|p| !p.is_empty()) {
            Some(path) => {
                let size = file_len(path).map_err(|e| {
                    format!("unable to read file part `{}` at {path}: {e}", part.name)
                })?;
                len += size;
                segments.push(Segment::File {
                    path: PathBuf::from(path),
                    len: size,
                });
            }
            // 有声明长度的没路径分片 = 这次请求上传的文件。长度只认声明值：通道里没有
            // 长度可量，`Content-Length` 与进度分母都得靠它。
            None => match part.size {
                Some(size) if uploads.is_some() => {
                    len += size;
                    claimed_uploads = true;
                    segments.push(Segment::Upload { len: size });
                }
                Some(_) => {
                    return Err(format!(
                        "file part `{}` declares a size but the request carries no uploaded bytes",
                        part.name
                    ))
                }
                None => push_bytes(&mut segments, &mut len, part.value.as_bytes().to_vec()),
            },
        }

        push_bytes(&mut segments, &mut len, b"\r\n".to_vec());
    }

    push_bytes(
        &mut segments,
        &mut len,
        format!("--{boundary}--\r\n").into_bytes(),
    );

    if !claimed_uploads {
        return Err("the request carries uploaded files, but its body declares none".to_string());
    }

    Ok(AssembledMultipart {
        content_type: content_type(boundary),
        body: OutboundBody {
            segments,
            len,
            uploads,
        },
    })
}

/// 一个文件的字节数。目录与不存在的路径都算错：前者读不出内容，后者是用户的文件被移走了。
fn file_len(path: &str) -> Result<u64, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".to_string());
    }
    Ok(meta.len())
}

#[cfg(test)]
#[path = "multipart_test.rs"]
mod tests;

#[cfg(test)]
#[path = "multipart_wire_test.rs"]
mod wire_tests;
