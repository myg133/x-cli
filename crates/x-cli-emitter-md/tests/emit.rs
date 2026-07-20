//! markdown emitter 的回归测试
//!
//! 网住 SKILL.md / endpoint md 的关键结构，防止后续重构里把 agent 用的关键信息
//! （调用约定、endpoint 链接、Python 调用示例）改没。

use std::path::PathBuf;
use x_cli_core::parse_openapi_str;
use x_cli_emitter_md::{MarkdownEmitter, SkillEmitter, SkillFormat};

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
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

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
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

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
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

    let ep = std::fs::read_to_string(out.join("endpoints").join("pet__get__pets_petId.md"))
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
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

    let ep = std::fs::read_to_string(out.join("endpoints").join("pet__post__pets.md"))
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
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit httpbin");
    assert!(out.join("SKILL.md").exists());
}

// ─────────────── B 阶段：resolved 树渲染 ───────────────

#[tokio::test]
async fn endpoint_with_request_body_renders_resolved_properties() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

    let ep = std::fs::read_to_string(out.join("endpoints").join("pet__post__pets.md"))
        .expect("read post endpoint md");

    // Pet 的属性必须出现在请求体 schema 表里
    assert!(
        ep.contains("`id`"),
        "expected `id` field in request body schema"
    );
    assert!(ep.contains("`name`"), "expected `name` field");
    assert!(ep.contains("`tag`"), "expected `tag` field");
    // name 标记为必填
    assert!(ep.contains("name") && ep.contains("✅"));
    // 字段类型是 scalar（string）
    assert!(
        ep.contains("`string`"),
        "expected scalar string type labels"
    );
}

#[tokio::test]
async fn response_schema_renders_too() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

    let ep = std::fs::read_to_string(out.join("endpoints").join("pet__get__pets_petId.md"))
        .expect("read getPet md");

    // 响应 200 schema 必须渲染 Pet 的字段
    assert!(ep.contains("响应 200"));
    assert!(ep.contains("`id`") || ep.contains("`name`"));
}

#[tokio::test]
async fn recursive_schema_does_not_loop_in_markdown() {
    // 自引用 Tree{value, children:[Tree]} - 必须渲染出来 + children 标 recursive
    let yaml = r#"
openapi: 3.1.0
info:
  title: Tree
  version: 1.0.0
paths:
  /tree:
    get:
      tags: [tree]
      operationId: getTree
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Tree'
components:
  schemas:
    Tree:
      type: object
      title: Tree
      required: [value]
      properties:
        value:
          type: string
        children:
          type: array
          items:
            $ref: '#/components/schemas/Tree'
"#;
    let spec = parse_openapi_str(yaml).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit tree");

    let ep = std::fs::read_to_string(out.join("endpoints").join("tree__get__tree.md"))
        .expect("read tree md");

    assert!(ep.contains("`value`"));
    assert!(ep.contains("`children`"));
    // 数组元素的类型必须是 array<`Tree`>，递归不爆栈
    assert!(ep.contains("array<`Tree`>"), "should render array<Tree>");
    // 不应该无限递归（如果爆栈这个测试根本到不了这里）
}

// ─────────────── C 阶段：工作流渲染 ───────────────

#[tokio::test]
async fn emits_workflow_files() {
    use x_cli_core::{parse_workflow_str, Workflow};

    let spec = parse_openapi_str(PETSTORE).expect("parse petstore");
    let workflow_yaml = r#"
name: 买一只宠物
description: |
  1. 创建宠物
  2. 读回来确认
inputs:
  - name: petId
    type: string
    description: 宠物 ID
steps:
  - name: create
    endpoint: pet__post__pets
    inputs:
      body:
        name: "fluffy"
        tag: "$input.petId"
  - name: read
    endpoint: pet__get__pets_petId
    inputs:
      path_params:
        petId: "$steps.create.response.body.id"
"#;
    let wf: Workflow = parse_workflow_str(workflow_yaml).expect("parse workflow");

    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(
            &spec,
            std::slice::from_ref(&wf),
            &out,
            SkillFormat::Markdown,
        )
        .await
        .expect("emit");

    // workflows/ 目录 + 2 份文件（md + yaml）
    let wf_dir = out.join("workflows");
    assert!(wf_dir.exists(), "workflows dir should exist");
    let files: Vec<_> = std::fs::read_dir(&wf_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 2, "expected 2 workflow files (md + yaml)");

    // 必须有 md 和 yaml
    let has_md = files
        .iter()
        .any(|f| f.path().extension().and_then(|s| s.to_str()) == Some("md"));
    let has_yaml = files
        .iter()
        .any(|f| f.path().extension().and_then(|s| s.to_str()) == Some("yaml"));
    assert!(has_md && has_yaml, "must have both .md and .yaml");

    // SKILL.md 必须含工作流段
    let skill = std::fs::read_to_string(out.join("SKILL.md")).expect("skill");
    assert!(skill.contains("## 工作流"));
    assert!(skill.contains("买一只宠物"));
}

#[tokio::test]
async fn workflow_md_includes_python_invocation() {
    use x_cli_core::{parse_workflow_str, Workflow};

    let spec = parse_openapi_str(PETSTORE).expect("parse petstore");
    let wf: Workflow = parse_workflow_str(
        r#"
name: create-and-read
steps:
  - name: create
    endpoint: pet__post__pets
    inputs:
      body:
        name: "fluffy"
  - name: read
    endpoint: pet__get__pets_petId
    inputs:
      path_params:
        petId: "$steps.create.response.body.id"
"#,
    )
    .expect("parse");

    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(
            &spec,
            std::slice::from_ref(&wf),
            &out,
            SkillFormat::Markdown,
        )
        .await
        .expect("emit");

    let body = std::fs::read_to_string(out.join("workflows").join("create-and-read.md"))
        .expect("read wf md");

    // 必须含两个 step 的 Python 调用
    assert!(body.contains("def call("));
    assert!(body.contains("create"));
    assert!(body.contains("read"));
    // inputs 引用要展开
    assert!(body.contains("$input.") || body.contains("$steps."));
    // 必须含 endpoint 引用
    assert!(body.contains("pet__post__pets"));
    assert!(body.contains("pet__get__pets_petId"));
}

#[tokio::test]
async fn workflow_step_describes_input_refs() {
    use x_cli_core::parse_workflow_str;

    let _spec = parse_openapi_str(PETSTORE).expect("parse");
    let wf = parse_workflow_str(
        r#"
name: demo
steps:
  - name: only
    endpoint: pet__get__pets_petId
    inputs:
      path_params:
        petId: "$input.petId"
"#,
    )
    .expect("parse");

    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(
            &_spec,
            std::slice::from_ref(&wf),
            &out,
            SkillFormat::Markdown,
        )
        .await
        .expect("emit");

    let body = std::fs::read_to_string(out.join("workflows").join("demo.md")).expect("read");

    // inputs 表格或 bullet 必须显式描述 $input.petId 引用
    assert!(
        body.contains("$input.petId") || body.contains("petId ← $input.petId"),
        "should describe $input.petId reference; body:\n{body}"
    );
}

// ─────────────── D 阶段：响应合并 + tag sanitize ───────────────

#[tokio::test]
async fn identical_response_schemas_get_merged_in_markdown() {
    // 多个响应 schema 相同（如 4xx/5xx 错误），渲染时合并成一行
    let yaml = r#"
openapi: 3.1.0
info:
  title: Errors
  version: 1.0.0
paths:
  /things:
    get:
      tags: [things]
      operationId: list
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                type: object
                properties:
                  id: { type: string }
        '400':
          description: Bad request
          content:
            application/json:
              schema:
                type: object
                properties:
                  message: { type: string }
        '401':
          description: Unauthorized
          content:
            application/json:
              schema:
                type: object
                properties:
                  message: { type: string }
        '500':
          description: Fatal
          content:
            application/json:
              schema:
                type: object
                properties:
                  message: { type: string }
"#;
    let spec = parse_openapi_str(yaml).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

    let body = std::fs::read_to_string(out.join("endpoints").join("things__get__things.md"))
        .expect("read");

    // 状态码行：400, 401, 500 合并成一行
    assert!(
        body.contains("**400, 401, 500**"),
        "expected merged status line; body:\n{body}"
    );
    // 不应该分别渲染 400/401/500
    assert!(
        !body.contains("**400** `application/json`"),
        "should not render 400 alone"
    );
    // schema 标题也合并
    assert!(
        body.contains("### 响应 400, 401, 500 schema"),
        "expected merged schema heading"
    );
    // 200 独立
    assert!(body.contains("**200**"));
}

#[tokio::test]
async fn merge_statuses_uses_range_when_continuous() {
    // 直接测 merge_statuses 函数：3 个连续状态码合并成 200-202
    // 通过观察一个内部连续的响应模式
    let yaml = r#"
openapi: 3.1.0
info:
  title: Range
  version: 1.0.0
paths:
  /x:
    get:
      tags: [x]
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema: { type: object }
        '201':
          description: created
          content:
            application/json:
              schema: { type: object }
        '202':
          description: accepted
          content:
            application/json:
              schema: { type: object }
"#;
    let spec = parse_openapi_str(yaml).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

    let body = std::fs::read_to_string(out.join("endpoints").join("x__get__x.md")).expect("read");

    // 200, 201, 202 三个 schema 一样 → 应该合并成 200-202
    assert!(
        body.contains("**200-202**"),
        "expected range merge '200-202'; body:\n{body}"
    );
}

#[tokio::test]
async fn tag_with_space_is_url_encoded_in_links() {
    // Superset-style: tag 名字含空格，markdown 链接里要 URL 编码
    let yaml = r#"
openapi: 3.1.0
info:
  title: TagTest
  version: 1.0.0
paths:
  /things:
    get:
      tags: ["My Tag"]
      operationId: list
      responses:
        '200':
          description: ok
"#;
    let spec = parse_openapi_str(yaml).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Markdown)
        .await
        .expect("emit");

    let skill = std::fs::read_to_string(out.join("SKILL.md")).expect("read skill");
    // 显示用真名（保留空格）
    assert!(
        skill.contains("`My Tag`"),
        "display name should be `My Tag`"
    );
    // 链接里要 URL 编码（%20）
    assert!(
        skill.contains("My%20Tag"),
        "link should URL-encode space as %20; got:\n{skill}"
    );
    // 文件名还是带空格（os 兼容的，不动）
    let file_path = out.join("endpoints").join("My Tag__get__things.md");
    assert!(
        file_path.exists(),
        "actual file should still have space in name"
    );
}

// ─────────────── E 阶段：Anthropic + OpenAI 格式 ───────────────

#[tokio::test]
async fn anthropic_format_emits_skill_md_with_frontmatter() {
    let spec = parse_openapi_str(PETSTORE).expect("parse petstore");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Anthropic)
        .await
        .expect("emit anthropic");

    // 应该只有一份 SKILL.md（不分子文件）
    let entries: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "Anthropic mode should produce only SKILL.md"
    );

    let skill = std::fs::read_to_string(out.join("SKILL.md")).expect("read skill");
    // 必须有 YAML frontmatter
    assert!(
        skill.starts_with("---\n"),
        "must start with YAML frontmatter"
    );
    assert!(skill.contains("name: Pet Store API"));
    assert!(skill.contains("description:"));
    // 不应该有分文件
    assert!(
        !out.join("endpoints").exists(),
        "no endpoints/ dir in anthropic mode"
    );
}

#[tokio::test]
async fn anthropic_description_includes_domain_summary() {
    // description 字段要让 Claude 知道何时用
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::Anthropic)
        .await
        .expect("emit");

    let skill = std::fs::read_to_string(out.join("SKILL.md")).expect("read");
    // description 应当包含端点数 + 业务域
    let in_frontmatter = skill.split("---\n").nth(1).expect("frontmatter");
    assert!(
        in_frontmatter.contains("5 个接口"),
        "should mention endpoint count"
    );
    assert!(in_frontmatter.contains("pet") && in_frontmatter.contains("store"));
}

#[tokio::test]
async fn openai_format_emits_functions_json() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::OpenAITools)
        .await
        .expect("emit openai");

    let path = out.join("functions.json");
    assert!(path.exists(), "functions.json should exist");
    let raw = std::fs::read_to_string(&path).expect("read");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");

    // 顶层是 { tools: [...] }
    let tools = v
        .get("tools")
        .and_then(|t| t.as_array())
        .expect("tools array");
    // 5 个 endpoint
    assert_eq!(tools.len(), 5, "5 endpoint = 5 tools");

    // 每个 tool 都有 type=function, function.{name, description, parameters}
    let first = &tools[0];
    assert_eq!(first.get("type").and_then(|t| t.as_str()), Some("function"));
    let f = first.get("function").expect("function");
    assert!(f.get("name").is_some());
    assert!(f.get("description").is_some());
    let params = f.get("parameters").expect("parameters");
    assert_eq!(params.get("type").and_then(|t| t.as_str()), Some("object"));
    assert!(params.get("properties").is_some());
}

#[tokio::test]
async fn openai_format_required_params_marked() {
    // petId 是 path required 参数 → 必须在 required 列表里
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &[], &out, SkillFormat::OpenAITools)
        .await
        .expect("emit");

    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("functions.json")).expect("read"))
            .expect("parse");
    let tools = v.get("tools").and_then(|t| t.as_array()).expect("tools");

    // 找 pet__get__pets_petId 这个 tool
    let tool = tools
        .iter()
        .find(|t| {
            t.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                == Some("pet__get__pets_petId")
        })
        .expect("pet get tool");
    let required = tool
        .get("function")
        .and_then(|f| f.get("parameters"))
        .and_then(|p| p.get("required"))
        .and_then(|r| r.as_array())
        .expect("required");
    let req_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    assert!(req_strs.contains(&"petId"), "petId must be required");
}

#[tokio::test]
async fn openai_format_workflows_also_become_tools() {
    use x_cli_core::{parse_workflow_str, Workflow};

    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let wf: Workflow = parse_workflow_str(
        r#"
name: create-and-read
steps:
  - name: create
    endpoint: pet__post__pets
"#,
    )
    .expect("parse wf");

    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(
            &spec,
            std::slice::from_ref(&wf),
            &out,
            SkillFormat::OpenAITools,
        )
        .await
        .expect("emit");

    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("functions.json")).expect("read"))
            .expect("parse");
    let tools = v.get("tools").and_then(|t| t.as_array()).expect("tools");

    // 5 endpoints + 1 workflow = 6
    assert_eq!(tools.len(), 6);
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("function")?.get("name")?.as_str())
        .collect();
    assert!(
        names.contains(&"workflow.create-and-read"),
        "workflow tool name: {names:?}"
    );
}

// ─────────────── F 阶段：depends_on 渲染 ───────────────

#[tokio::test]
async fn workflow_md_renders_depends_on_field() {
    use x_cli_core::parse_workflow_str;

    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let wf = parse_workflow_str(
        r#"
name: with-deps
steps:
  - name: first
    endpoint: pet__get__pets
  - name: second
    endpoint: pet__get__pets_petId
    depends_on: [first]
"#,
    )
    .expect("parse");

    let out = temp_out();
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(
            &spec,
            std::slice::from_ref(&wf),
            &out,
            SkillFormat::Markdown,
        )
        .await
        .expect("emit");

    let body = std::fs::read_to_string(out.join("workflows").join("with-deps.md")).expect("read");

    // 第一个 step 无依赖
    assert!(
        body.contains("depends_on: —"),
        "first step should have no deps marker"
    );
    // 第二个 step 有依赖
    assert!(
        body.contains("depends_on: first"),
        "second step should show dep on 'first'"
    );
}
