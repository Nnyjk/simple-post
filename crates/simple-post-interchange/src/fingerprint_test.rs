//! 指纹的输入与排除项——「改名不造副本」与「两种格式互相认得」的可执行版本。

use simple_post_core::domain::{AuthConfig, HttpMethod, KeyValue, RequestBody};

use super::{normalize_path, FingerprintSet, RequestIdentity};
use crate::ir::IrEndpoint;

/// 一个请求的全部可变部分，用来证明哪些字段**不**进指纹。
fn fully_decorated_ep() -> IrEndpoint {
    let mut ep = IrEndpoint::new("列表用户", HttpMethod::Post, "/users");
    ep.description = Some("一段描述".into());
    ep.headers = vec![kv("X-Trace", "abc")];
    ep.body = RequestBody {
        mode: simple_post_core::domain::BodyMode::Json,
        content: "{\"name\":\"a\"}".into(),
        schema: None,
    };
    ep.auth = AuthConfig {
        kind: simple_post_core::domain::AuthType::Bearer,
        bearer: Some("tok".into()),
        ..Default::default()
    };
    ep.tags = vec!["用户".into()];
    ep
}

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        id: String::new(),
        key: key.into(),
        value: value.into(),
        enabled: true,
        description: None,
    }
}

#[test]
fn renaming_does_not_change_the_fingerprint() {
    let base = fully_decorated_ep();
    let mut renamed = base.clone();
    renamed.name = "改名之后的名字".into();
    renamed.description = Some("换一段描述".into());
    renamed.tags = vec!["另一个标签".into()];

    assert_eq!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&renamed).fingerprint()
    );
}

#[test]
fn auth_does_not_change_the_fingerprint() {
    let base = fully_decorated_ep();
    let mut no_auth = base.clone();
    no_auth.auth = AuthConfig::default();

    assert_eq!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&no_auth).fingerprint()
    );
}

#[test]
fn headers_do_not_change_the_fingerprint_but_body_does() {
    let base = fully_decorated_ep();

    // header 是传输层的修饰，不是接口的身份。
    let mut no_headers = base.clone();
    no_headers.headers.clear();
    assert_eq!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&no_headers).fingerprint()
    );

    // body 反过来必须参与：一份集合里两个 `POST /users` 配不同 payload 是常见写法，
    // 把它们判成同一个就是丢请求，不是去重。
    let mut other_body = base.clone();
    other_body.body.content = "{\"name\":\"b\"}".into();
    assert_ne!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&other_body).fingerprint()
    );

    // 同一段文本挂在不同 mode 下也是不同的请求。
    let mut as_raw = base.clone();
    as_raw.body.mode = simple_post_core::domain::BodyMode::Raw;
    assert_ne!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&as_raw).fingerprint()
    );
}

#[test]
fn method_does_change_the_fingerprint() {
    let base = fully_decorated_ep();
    let mut other = base.clone();
    other.method = HttpMethod::Get;

    assert_ne!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&other).fingerprint()
    );
}

#[test]
fn postman_shape_and_openapi_shape_agree() {
    // 同一份接口经两种外来格式建出来的 IR。差别只在书写习惯上：
    // Postman 的 raw 少一个前导斜杠、query 顺序不同、名字与描述另有一套。
    let mut postman = fully_decorated_ep();
    postman.name = "Get Users".into();
    postman.path = "users".into();
    postman.params = vec![kv("limit", "10"), kv("page", "1")];

    let mut openapi = fully_decorated_ep();
    openapi.name = "用户列表".into();
    openapi.path = "/users/".into();
    openapi.params = vec![kv("page", "1"), kv("limit", "10")];

    assert_eq!(
        RequestIdentity::of_ir(&postman).fingerprint(),
        RequestIdentity::of_ir(&openapi).fingerprint()
    );
}

#[test]
fn a_disabled_param_does_not_count_but_a_differing_value_does() {
    let base = fully_decorated_ep();
    let mut disabled = base.clone();
    disabled.params = vec![KeyValue {
        enabled: false,
        ..kv("trace", "1")
    }];
    assert_eq!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&disabled).fingerprint()
    );

    let mut other_value = base.clone();
    other_value.params = vec![kv("trace", "2")];
    assert_ne!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&other_value).fingerprint()
    );
}

#[test]
fn a_differing_path_param_value_makes_a_different_request() {
    // `path` 里留着 `{id}` 这个占位符，两个请求只差 id 的值时路径文本一模一样。
    // 不把路径参数的值算进指纹，第二个就会被当成重复而跳过 —— 那是丢请求，不是去重。
    let mut first = fully_decorated_ep();
    first.path = "/users/{id}".into();
    first.path_params = vec![kv("id", "1")];

    let mut second = first.clone();
    second.path_params = vec![kv("id", "2")];

    assert_ne!(
        RequestIdentity::of_ir(&first).fingerprint(),
        RequestIdentity::of_ir(&second).fingerprint(),
        "路径参数的值不同就是不同的请求"
    );

    let mut set = FingerprintSet::new();
    assert!(!set.seen(&RequestIdentity::of_ir(&first)));
    assert!(
        !set.seen(&RequestIdentity::of_ir(&second)),
        "只差路径参数值的第二个请求不该被当成重复"
    );
}

#[test]
fn a_disabled_path_param_does_not_count() {
    // 停用的路径参数不进请求，故它不该改变身份。
    let mut base = fully_decorated_ep();
    base.path = "/users/{id}".into();

    let mut disabled = base.clone();
    disabled.path_params = vec![KeyValue {
        enabled: false,
        ..kv("id", "1")
    }];

    assert_eq!(
        RequestIdentity::of_ir(&base).fingerprint(),
        RequestIdentity::of_ir(&disabled).fingerprint()
    );
}

#[test]
fn a_query_param_and_a_path_param_do_not_collide() {
    // query 段与 path_params 段是**两段**，不是一段。分界说不清时，query 是
    // `a=1,b=2`、path_params 是 `b=2` 的两个请求会拼出同一条字节流，拿到同一个
    // 指纹 —— 导入时第二个被当成重复而跳过，那是丢请求，不是去重。
    let mut as_query = fully_decorated_ep();
    as_query.path = "/items".into();
    as_query.params = vec![kv("a", "1"), kv("b", "2")];

    let mut as_path_param = fully_decorated_ep();
    as_path_param.path = "/items".into();
    as_path_param.params = vec![kv("a", "1")];
    as_path_param.path_params = vec![kv("b", "2")];

    assert_ne!(
        RequestIdentity::of_ir(&as_query).fingerprint(),
        RequestIdentity::of_ir(&as_path_param).fingerprint(),
        "同一个值放在 query 里与放在路径参数里，是两个不同的请求"
    );

    let mut set = FingerprintSet::new();
    assert!(!set.seen(&RequestIdentity::of_ir(&as_query)));
    assert!(
        !set.seen(&RequestIdentity::of_ir(&as_path_param)),
        "放在路径参数里的那个请求不该被当成重复而跳过"
    );
}

#[test]
fn a_param_value_containing_a_newline_does_not_collide() {
    // 值里带换行时，一个参数能伪装成两个参数：按换行切开是 `a=1` 与 `b=2`。
    // 每一段都带自己的长度时，长度就是这一段的终止标记，故这两种写法算出不同的
    // 指纹。
    let mut one_param = fully_decorated_ep();
    one_param.params = vec![kv("a", "1\nb=2")];

    let mut two_params = fully_decorated_ep();
    two_params.params = vec![kv("a", "1"), kv("b", "2")];

    assert_ne!(
        RequestIdentity::of_ir(&one_param).fingerprint(),
        RequestIdentity::of_ir(&two_params).fingerprint(),
        "一个值里带换行的参数不该与两个参数撞成同一个指纹"
    );
}

#[test]
fn normalize_path_is_idempotent_and_keeps_case() {
    for raw in [
        "users",
        "/users",
        "/users/",
        "users/",
        "//users//1//",
        " /users/1 ",
    ] {
        let once = normalize_path(raw);
        assert_eq!(once, normalize_path(&once), "not idempotent for {raw:?}");
    }
    assert_eq!(normalize_path("users"), "/users");
    assert_eq!(normalize_path("/users/"), "/users");
    assert_eq!(normalize_path("//users//1//"), "/users/1");
    assert_eq!(normalize_path("/"), "/");
    assert_eq!(normalize_path(""), "/");
    // 路径大小写有语义，不能折叠。
    assert_eq!(normalize_path("/Users"), "/Users");
}

#[test]
fn the_set_recognises_a_repeated_request() {
    let mut set = FingerprintSet::new();
    let id = RequestIdentity::of_ir(&fully_decorated_ep());
    assert!(!set.seen(&id));
    assert!(set.seen(&id), "同一个 IR 内部的重复项第二次就该被认出来");
}

#[test]
fn the_set_recognises_an_already_stored_endpoint() {
    // 构造一条已落库的行，走 `of_detail` 那条边。
    let detail = simple_post_core::domain::EndpointDetail {
        node_id: "sp_1".into(),
        method: HttpMethod::Post,
        url: "/users".into(),
        params: vec![],
        path_params: vec![],
        headers: vec![kv("X-Trace", "abc")],
        // 与 `fully_decorated_ep` 的 body 一致：落库不改 body，两侧才算出同一个身份。
        body: RequestBody {
            mode: simple_post_core::domain::BodyMode::Json,
            content: "{\"name\":\"a\"}".into(),
            schema: None,
        },
        auth: AuthConfig::default(),
        base_url_override: None,
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    };

    let mut set = FingerprintSet::new();
    set.absorb_details([&detail]);

    let incoming = fully_decorated_ep();
    assert!(
        set.seen(&RequestIdentity::of_ir(&incoming)),
        "改了名字 / 描述 / 标签 / auth / header 的同一个接口仍应被认出来"
    );

    // 但改了 body 是另一个请求：已落库的那条不能把它吃掉。
    let mut different_payload = fully_decorated_ep();
    different_payload.body.content = "{\"name\":\"b\"}".into();
    let mut fresh = FingerprintSet::new();
    fresh.absorb_details([&detail]);
    assert!(
        !fresh.seen(&RequestIdentity::of_ir(&different_payload)),
        "同路由不同 payload 必须落成两个 endpoint"
    );
}
