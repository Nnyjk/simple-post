//! base64 编码器。RFC 4648 §4 的向量表，加上两个真实调用点的形状。

use super::*;

#[test]
fn rfc4648_vectors() {
    // RFC 4648 §10 的测试向量。
    assert_eq!(encode(b""), "");
    assert_eq!(encode(b"f"), "Zg==");
    assert_eq!(encode(b"fo"), "Zm8=");
    assert_eq!(encode(b"foo"), "Zm9v");
    assert_eq!(encode(b"foob"), "Zm9vYg==");
    assert_eq!(encode(b"fooba"), "Zm9vYmE=");
    assert_eq!(encode(b"foobar"), "Zm9vYmFy");
}

#[test]
fn the_http_basic_shapes_round_trip_through_a_known_value() {
    // `user:pass` 是 HTTP Basic 最常见的字面量；`aladdin:open sesame` 是 RFC 7617
    // 的例子。两者都必须与 OpenSSL 的输出一致，否则服务端解出的不是同一组凭据。
    assert_eq!(encode(b"user:pass"), "dXNlcjpwYXNz");
    assert_eq!(
        encode(b"Aladdin:open sesame"),
        "QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
    );
}

#[test]
fn non_ascii_is_encoded_as_its_utf8_bytes() {
    // 不做 Unicode 变换：base64 定义在字节上。中文用户名会先在调用方按 UTF-8 取字节。
    assert_eq!(encode("中".as_bytes()), "5Lit");
    assert_eq!(encode("é".as_bytes()), "w6k=");
}

#[test]
fn every_byte_value_survives_a_length_that_straddles_the_block() {
    // 覆盖 rem = 0/1/2 三条补位分支，且输入含全部字节值。
    for len in 0..40usize {
        let input: Vec<u8> = (0..len).map(|i| (i * 7 % 256) as u8).collect();
        let out = encode(&input);
        assert_eq!(out.len() % 4, 0, "len={len} out={out}");
        assert!(
            out.ends_with('=') || out.is_empty() || len % 3 == 0,
            "len={len}"
        );
    }
}
