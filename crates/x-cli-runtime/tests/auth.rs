//! Auth profile 构造的单元测试

use x_cli_runtime::build_auth_profile;

#[test]
fn empty_inputs_produces_empty_profile() {
    let p = build_auth_profile(&[], &[]).expect("build");
    assert!(p.headers.is_empty());
}

#[test]
fn bearer_token_becomes_authorization_header() {
    let p = build_auth_profile(&["abc123".to_string()], &[]).expect("build");
    assert_eq!(
        p.headers.get("Authorization").map(|s| s.as_str()),
        Some("Bearer abc123")
    );
}

#[test]
fn multiple_bearers_last_wins() {
    let p = build_auth_profile(&["first".to_string(), "second".to_string()], &[]).expect("build");
    assert_eq!(
        p.headers.get("Authorization").map(|s| s.as_str()),
        Some("Bearer second")
    );
}

#[test]
fn custom_header_is_passthrough() {
    let p = build_auth_profile(
        &[],
        &["X-API-Key=secret".to_string(), "X-Tenant=acme".to_string()],
    )
    .expect("build");
    assert_eq!(
        p.headers.get("X-API-Key").map(|s| s.as_str()),
        Some("secret")
    );
    assert_eq!(p.headers.get("X-Tenant").map(|s| s.as_str()), Some("acme"));
}

#[test]
fn header_overrides_bearer() {
    // 后传入的覆盖先传入的：先 bearer 再 custom Authorization=...
    let p = build_auth_profile(
        &["token-a".to_string()],
        &["Authorization=Bearer token-b".to_string()],
    )
    .expect("build");
    assert_eq!(
        p.headers.get("Authorization").map(|s| s.as_str()),
        Some("Bearer token-b")
    );
}

#[test]
fn malformed_header_string_is_rejected() {
    // 没有 = 分隔符
    let err = build_auth_profile(&[], &["JUST-A-KEY".to_string()]).expect_err("should fail");
    assert!(err.to_string().contains("KEY=VALUE"));
}

#[test]
fn empty_key_is_rejected() {
    let err = build_auth_profile(&[], &["=value".to_string()]).expect_err("should fail");
    assert!(err.to_string().contains("key 不能为空"));
}
