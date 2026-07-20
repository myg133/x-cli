//! JSON-RPC transport 的回归测试
//!
//! 用 tokio::io::duplex 模拟 stdio，验证 ping、call 错误路径、protocol 错误处理。

use std::sync::Arc;
use tokio::io::{duplex, AsyncWriteExt};
use x_cli_core::ir::ApiSpec;
use x_cli_core::parse_openapi_str;
use x_cli_runtime::{serve, AuthProfile, HttpCaller};

const PETSTORE: &str = include_str!("fixtures/petstore.yaml");

fn spec() -> Arc<ApiSpec> {
    Arc::new(parse_openapi_str(PETSTORE).expect("parse petstore"))
}

async fn round_trip(spec: Arc<ApiSpec>, request: &str) -> Vec<String> {
    let (mut client_write, server_read) = duplex(4096);
    let (server_write, mut client_read) = duplex(4096);

    let caller = HttpCaller::new(AuthProfile::default()).expect("caller");
    let serve_task = tokio::spawn(async move {
        serve(
            server_read,
            server_write,
            spec,
            std::collections::BTreeMap::new(),
            None,
            caller,
        )
        .await;
    });

    // 写请求 + 关闭写端触发 EOF
    client_write.write_all(request.as_bytes()).await.unwrap();
    client_write.write_all(b"\n").await.unwrap();
    drop(client_write);

    // 读所有响应直到 EOF
    let mut buf = Vec::new();
    use tokio::io::AsyncReadExt;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client_read.read_to_end(&mut buf),
    )
    .await
    .expect("response timed out");

    serve_task.await.unwrap();

    let s = String::from_utf8(buf).expect("utf8");
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[tokio::test]
async fn ping_round_trip() {
    let resp = round_trip(spec(), r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).await;
    assert_eq!(resp.len(), 1);
    assert!(resp[0].contains("\"pong\":true"));
    assert!(resp[0].contains("\"id\":1"));
}

#[tokio::test]
async fn call_unknown_endpoint_returns_endpoint_not_found() {
    let req = r#"{"jsonrpc":"2.0","id":7,"method":"call","params":{"endpoint_id":"nonexistent","path_params":{},"query":{},"headers":{},"body":null}}"#;
    let resp = round_trip(spec(), req).await;
    assert_eq!(resp.len(), 1);
    // 错误码 -32001 (ENDPOINT_NOT_FOUND)
    assert!(resp[0].contains("\"code\":-32001"), "got: {}", resp[0]);
    assert!(resp[0].contains("nonexistent"));
}

#[tokio::test]
async fn invalid_json_returns_parse_error() {
    let resp = round_trip(spec(), r#"this is not json"#).await;
    assert_eq!(resp.len(), 1);
    // 错误码 -32700 (PARSE_ERROR)
    assert!(resp[0].contains("\"code\":-32700"), "got: {}", resp[0]);
}

#[tokio::test]
async fn empty_lines_are_ignored() {
    // 发一个空行 + 一个 ping：服务应该跳过空行、正常响应 ping
    let resp = round_trip(
        spec(),
        "\n\n   \n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n",
    )
    .await;
    assert_eq!(resp.len(), 1, "expected only one response, got: {resp:?}");
    assert!(resp[0].contains("\"pong\":true"));
}
