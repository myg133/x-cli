//! `auth.yaml` schema 的集成测试
//!
//! 用例:合法配置(bearer / login / 全字段) + 非法配置(YAML 错 / 缺字段 / 空 bearer / 错 version)。

use x_cli_core::{parse_auth_config_str, AuthParseError, TokenSource};

// 简单 bearer
const BEARER_SIMPLE: &str = include_str!("fixtures/auth-bearer-simple.yaml");
// login 基础形态
const LOGIN_BASIC: &str = include_str!("fixtures/auth-login-basic.yaml");
// login 全字段(headers + refresh + 各种 path)
const LOGIN_FULL: &str = include_str!("fixtures/auth-login-full.yaml");

#[test]
fn parse_bearer_simple() {
    let cfg = parse_auth_config_str(BEARER_SIMPLE).expect("parse");
    assert_eq!(cfg.version, 1);
    match &cfg.token {
        TokenSource::Bearer { bearer } => assert_eq!(bearer, "eyJhbGc.eyJzdWI.signature"),
        _ => panic!("expected Bearer"),
    }
}

#[test]
fn parse_login_basic_uses_defaults() {
    let cfg = parse_auth_config_str(LOGIN_BASIC).expect("parse");
    match cfg.token {
        TokenSource::Login { login } => {
            assert_eq!(login.request.method, "POST"); // default
            assert_eq!(login.response.token_path, "access_token"); // default
            assert!(login.refresh.on_401); // default true
            assert!(!login.refresh.proactive); // default false
            assert!(login.request.url.is_none()); // 缺省 = 用 skill base-url
        }
        _ => panic!("expected Login"),
    }
}

#[test]
fn parse_login_full() {
    let cfg = parse_auth_config_str(LOGIN_FULL).expect("parse");
    let login = match cfg.token {
        TokenSource::Login { login } => login,
        _ => panic!("expected Login"),
    };
    assert_eq!(
        login.request.url.as_deref(),
        Some("https://api.example.com/auth/login")
    );
    assert_eq!(login.request.method, "POST");
    assert_eq!(
        login.request.body,
        serde_json::json!({ "username": "admin", "password": "secret" })
    );
    assert_eq!(
        login.response.token_path,
        "data.access_token" // dotted path 自定义
    );
    assert_eq!(
        login.response.expires_in_path.as_deref(),
        Some("data.expires_in")
    );
    assert_eq!(
        login.response.refresh_token_path.as_deref(),
        Some("data.refresh_token")
    );
    assert!(!login.refresh.on_401); // 显式关掉
}

#[test]
fn reject_missing_token() {
    let yaml = r#"
version: 1
"#;
    let err = parse_auth_config_str(yaml).unwrap_err();
    assert!(matches!(err, AuthParseError::Yaml(_)), "got {err:?}");
}

#[test]
fn reject_empty_bearer() {
    let yaml = r#"
version: 1
token:
  kind: bearer
  bearer: ""
"#;
    let err = parse_auth_config_str(yaml).unwrap_err();
    assert!(matches!(err, AuthParseError::Invalid(_)), "got {err:?}");
}

#[test]
fn reject_unknown_kind() {
    let yaml = r#"
version: 1
token:
  kind: oauth2
  client_id: x
"#;
    let err = parse_auth_config_str(yaml).unwrap_err();
    assert!(matches!(err, AuthParseError::Yaml(_)), "got {err:?}");
}

#[test]
fn reject_on_401_with_proactive() {
    let yaml = r#"
version: 1
token:
  kind: login
  request:
    body: { user: a, pass: b }
  response: {}
  refresh:
    on_401: true
    proactive: true
"#;
    let err = parse_auth_config_str(yaml).unwrap_err();
    assert!(matches!(err, AuthParseError::Invalid(_)), "got {err:?}");
}
