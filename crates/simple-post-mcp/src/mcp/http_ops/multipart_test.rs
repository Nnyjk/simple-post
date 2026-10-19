use super::*;

fn parts(json: &str) -> Vec<MultipartPart> {
    parse_parts(json)
}

#[test]
fn boundary_in_body_and_content_type_is_the_same_string() {
    let b = new_boundary();
    let ct = content_type(&b);
    let body = build_body(&parts(r#"[{"name":"f","value":"v"}]"#), &b);

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
    let body = build_body(
        &parts(r#"[{"name":"a","value":"1"},{"name":"b","value":"2"}]"#),
        b,
    );
    assert_eq!(
        body,
        "--BOUND\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n\
         --BOUND\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\n2\r\n--BOUND--\r\n"
    );
}

#[test]
fn filename_makes_it_a_file_part_and_defaults_its_content_type() {
    let body = build_body(
        &parts(r#"[{"name":"f","value":"data","filename":"a.txt"}]"#),
        "B",
    );
    assert!(body.contains("Content-Disposition: form-data; name=\"f\"; filename=\"a.txt\""));
    assert!(body.contains("Content-Type: application/octet-stream"));
}

#[test]
fn explicit_content_type_wins_over_the_file_default() {
    let body = build_body(
        &parts(r#"[{"name":"f","value":"d","filename":"a.png","contentType":"image/png"}]"#),
        "B",
    );
    assert!(body.contains("Content-Type: image/png"));
    assert!(!body.contains("application/octet-stream"));
}

#[test]
fn plain_field_gets_no_content_type() {
    let body = build_body(&parts(r#"[{"name":"a","value":"1"}]"#), "B");
    assert!(!body.contains("Content-Type:"));
}

#[test]
fn header_params_escape_quotes_and_strip_crlf() {
    // CR/LF 留着就是往请求头里注入一个新头
    let body = build_body(
        &parts(r#"[{"name":"a\"b\r\nX-Evil: 1","value":"v","filename":"c\\d.txt"}]"#),
        "B",
    );
    assert!(body.contains("name=\"a\\\"bX-Evil: 1\""));
    assert!(body.contains("filename=\"c\\\\d.txt\""));
    assert!(!body.contains("X-Evil: 1\r\n"));
}

#[test]
fn zero_parts_still_produces_a_well_formed_body() {
    assert_eq!(build_body(&[], "B"), "--B--\r\n");
}

#[test]
fn malformed_json_yields_zero_parts() {
    assert!(parse_parts("not json").is_empty());
    assert!(parse_parts("").is_empty());
    assert!(parse_parts("{}").is_empty());
}

#[test]
fn a_wrongly_typed_field_discards_the_whole_list() {
    // 与 TS 侧同形：逐字段兜底会让同一个畸形输入在两侧得到不同的分片
    assert!(parse_parts(r#"[{"name":"a","value":10}]"#).is_empty());
    assert!(parse_parts(r#"[{"name":1,"value":"x"}]"#).is_empty());
    assert!(parse_parts(r#"["a"]"#).is_empty());
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
    let body = build_body(&parsed, "B");
    assert!(body.contains("name=\"b\""));
    assert!(!body.contains("filename="));
    assert!(!body.contains("Content-Type:"));
}

#[test]
fn camel_case_content_type_is_the_wire_field() {
    let parsed = parts(r#"[{"name":"f","value":"d","contentType":"image/png"}]"#);
    assert_eq!(parsed[0].content_type.as_deref(), Some("image/png"));
}

// ============================================================
// apply：Content-Type 覆盖 + body 装配
// ============================================================

#[test]
fn apply_overrides_a_user_set_content_type_because_the_body_is_ours() {
    // 其余模式的 body 是用户手写的，用户那条 Content-Type 有机会描述它；multipart
    // 的 body 是我们用 boundary 框出来的，用户那条值描述不了它
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    let body = apply(&mut headers, r#"[{"name":"a","value":"1"}]"#);

    assert_eq!(headers.len(), 1, "the header is replaced, not duplicated");
    assert_eq!(
        headers[0].0, "content-type",
        "the user's header name casing is kept"
    );
    let b = headers[0]
        .1
        .strip_prefix("multipart/form-data; boundary=")
        .expect("a multipart Content-Type");
    assert!(body.starts_with(&format!("--{b}\r\n")));
    assert!(body.ends_with(&format!("--{b}--\r\n")));
}

#[test]
fn apply_appends_a_content_type_when_the_user_set_none() {
    let mut headers: Vec<(String, String)> =
        vec![("accept".to_string(), "application/json".into())];
    let body = apply(&mut headers, r#"[{"name":"a","value":"1"}]"#);

    assert_eq!(headers.len(), 2);
    assert_eq!(headers[1].0, "Content-Type");
    let b = headers[1]
        .1
        .strip_prefix("multipart/form-data; boundary=")
        .expect("a multipart Content-Type");
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
/// `http-build-multipart.test.ts` 断言同一份字符串。
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
        r#"{"name":"emptyFile","value":"x","filename":"b.bin"}]"#
    );

    let expected = [
        format!("--{B}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\n报告"),
        format!("--{B}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\nContent-Type: application/octet-stream\r\n\r\nbinary-ish"),
        format!("--{B}\r\nContent-Disposition: form-data; name=\"note\"\r\nContent-Type: text/plain\r\n\r\n"),
        // 分片名里的引号与反斜杠被转义，CR/LF 整个被剥掉——留着就是往头里注入一个新头。
        format!("--{B}\r\nContent-Disposition: form-data; name=\"we\\\"ird\\\\nameinjected\"\r\n\r\nv"),
        format!("--{B}\r\nContent-Disposition: form-data; name=\"emptyFile\"; filename=\"b.bin\"\r\nContent-Type: application/octet-stream\r\n\r\nx"),
    ]
    .join("\r\n")
        + &format!("\r\n--{B}--\r\n");

    assert_eq!(build_body(&parts(content), B), expected);
}
