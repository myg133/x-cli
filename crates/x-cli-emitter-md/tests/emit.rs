//! markdown emitter 的回归测试
//!
//! 网住 SKILL.md / endpoint md 的关键结构，防止后续重构里把 agent 用的关键信息
//! （调用约定、endpoint 链接、Python 调用示例）改没。

use std::path::PathBuf;
use x_cli_core::parse_openapi_str;
use x_cli_emitter_md::{MarkdownEmitter, SkillEmitter};

const PETSTORE: &str = include_str!("fixtures/petstore.yaml");
const HTTPBIN: &str = include_str!("fixtures/httpbin.yaml");

/// 测试用临时目录：cargo test 每次新一个
fn temp_out() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "x-cli-emitter-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn emits_skill_index_with_calling_convention() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter.emit(&spec, &out).await.expect("emit");

    let skill_md = std::fs::read_to_string(out.join("SKILL.md")).expect("read SKILL.md");

    // 标题
    assert!(skill_md.contains("# Pet Store API — x-cli skill"));
    // base url
    assert!(skill_md.contains("https://petstore.example.com/v1"));
    // 调用约定：JSON-RPC over stdio
    assert!(skill_md.contains("JSON-RPC 2.0 over stdio"));
    assert!(skill_md.contains("x serve"));
    // 业务域 + 端点链接
    assert!(skill_md.contains("`pet`"));
    assert!(skill_md.contains("`store`"));
    assert!(skill_md.contains("pet__get__pets_petId"));
    assert!(skill_md.contains("./endpoints/pet__get__pets_petId.md"));
}

#[tokio::test]
async fn emits_endpoint_files() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter.emit(&spec, &out).await.expect("emit");

    // 5 个端点文件
    for id in [
        "pet__get__pets",
        "pet__post__pets",
        "pet__get__pets_petId",
        "store__post__store_orders",
        "store__get__store_orders_orderId",
    ] {
        let path = out.join("endpoints").join(format!("{id}.md"));
        assert!(path.exists(), "expected endpoint file: {}", path.display());
    }
}

#[tokio::test]
async fn endpoint_md_contains_python_invocation_example() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter.emit(&spec, &out).await.expect("emit");

    let ep = std::fs::read_to_string(
        out.join("endpoints").join("pet__get__pets_petId.md"),
    )
    .expect("read endpoint md");

    // 必须含 python 调用示例
    assert!(ep.contains("```python"));
    assert!(ep.contains("subprocess"));
    assert!(ep.contains("\"jsonrpc\""));
    assert!(ep.contains("\"method\": \"call\""));
    assert!(ep.contains("\"endpoint_id\": \"pet__get__pets_petId\""));
    // path_params 占位
    assert!(ep.contains("\"path_params\""));
    assert!(ep.contains("petId"));
    // 操作说明
    assert!(ep.contains("**`GET /pets/{petId}`**"));
    assert!(ep.contains("宠物 ID"));
}

#[tokio::test]
async fn endpoint_md_with_body_has_body_placeholder() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter.emit(&spec, &out).await.expect("emit");

    let ep = std::fs::read_to_string(
        out.join("endpoints").join("pet__post__pets.md"),
    )
    .expect("read post endpoint md");

    assert!(ep.contains("**`POST /pets`**"));
    assert!(ep.contains("\"body\""));
    assert!(ep.contains("application/json"));
}

#[tokio::test]
async fn httpbin_emit_succeeds() {
    let spec = parse_openapi_str(HTTPBIN).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter.emit(&spec, &out).await.expect("emit httpbin");
    assert!(out.join("SKILL.md").exists());
}
