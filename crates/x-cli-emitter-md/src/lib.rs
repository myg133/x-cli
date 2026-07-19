//! x-cli-emitter-md: 把 IR 渲染为 markdown skill 描述
//!
//! 目录结构：
//! ```text
//! {out_dir}/
//!   SKILL.md                 # 总索引（领域、endpoints 列表、调用约定）
//!   endpoints/
//!     <endpoint_id>.md       # 单个 endpoint 的详细描述 + agent 调用示例
//! ```
//!
//! agent 加载这个目录后，能像读 API 文档一样使用 skill。

#![warn(missing_docs)]

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use x_cli_core::ir::{ApiSpec, Endpoint, HttpMethod, InputRef, ParamLocation, ResolvedSchema, SchemaRef, Workflow, WorkflowStep};

/// SkillEmitter trait — 不同平台各自实现
#[async_trait]
pub trait SkillEmitter {
    /// 把 IR 渲染到 out_dir
    async fn emit(&self, spec: &ApiSpec, workflows: &[Workflow], out_dir: &Path) -> Result<()>;
}

/// markdown emitter
pub struct MarkdownEmitter;

impl MarkdownEmitter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MarkdownEmitter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SkillEmitter for MarkdownEmitter {
    async fn emit(&self, spec: &ApiSpec, workflows: &[Workflow], out_dir: &Path) -> Result<()> {
        fs::create_dir_all(out_dir).context("create out_dir")?;
        fs::create_dir_all(out_dir.join("endpoints")).context("create endpoints dir")?;

        // SKILL.md 总索引
        let index = render_index(spec, workflows);
        fs::write(out_dir.join("SKILL.md"), index).context("write SKILL.md")?;

        // 每个 endpoint 一份
        for ep in spec.endpoints.values() {
            let body = render_endpoint(ep, spec);
            let safe = sanitize_filename(&ep.id);
            fs::write(out_dir.join("endpoints").join(format!("{safe}.md")), body)
                .context("write endpoint md")?;
        }

        // 每个 workflow 一份
        if !workflows.is_empty() {
            fs::create_dir_all(out_dir.join("workflows")).context("create workflows dir")?;
            for wf in workflows {
                let body = render_workflow(wf, spec);
                let safe = sanitize_filename(&wf.name);
                fs::write(
                    out_dir.join("workflows").join(format!("{safe}.md")),
                    body,
                )
                .context("write workflow md")?;
            }
        }

        Ok(())
    }
}

fn render_index(spec: &ApiSpec, workflows: &[Workflow]) -> String {
    let mut s = String::new();
    s.push_str(&format!("# {} — x-cli skill\n\n", spec.title));
    s.push_str(&format!("> 自动生成自 OpenAPI {}，由 x-cli 渲染。请勿手动修改。\n\n", spec.version));

    if let Some(desc) = &spec.description {
        s.push_str(&format!("{desc}\n\n"));
    }
    if let Some(url) = &spec.base_url {
        s.push_str(&format!("**Base URL**: `{url}`\n\n"));
    }

    s.push_str("## 调用约定\n\n");
    s.push_str("skill 通过 JSON-RPC 2.0 over stdio 调 x-cli：\n\n");
    s.push_str("```text\n");
    s.push_str("x serve          # 启动 x-cli 的 stdio JSON-RPC 服务\n");
    s.push_str("                 # skill 端按行写 JSON 请求，按行读 JSON 响应\n");
    s.push_str("```\n\n");
    s.push_str("请求示例：\n\n");
    s.push_str("```json\n");
    s.push_str(r#"{"jsonrpc":"2.0","id":1,"method":"call","params":{"endpoint_id":"<id>","path_params":{},"query":{},"headers":{},"body":{}}}"#);
    s.push_str("\n```\n\n");

    s.push_str("## 业务域\n\n");
    for d in &spec.domains {
        s.push_str(&format!("### `{}`（{} 个接口）\n\n", d.name, d.endpoint_ids.len()));
        for id in &d.endpoint_ids {
            if let Some(ep) = spec.endpoints.get(id) {
                s.push_str(&format!(
                    "- [`{id}`](./endpoints/{safe}.md) — `{} {}`{}\n",
                    ep.method.as_str(),
                    ep.path,
                    ep.summary
                        .as_deref()
                        .map(|x| format!(" — {x}"))
                        .unwrap_or_default(),
                    safe = sanitize_filename(id),
                ));
            }
        }
        s.push('\n');
    }

    // 工作流段（C 阶段）
    if !workflows.is_empty() {
        s.push_str("## 工作流\n\n");
        s.push_str("工作流把多个接口串成多步任务，agent 按步骤顺序调用。\n\n");
        for wf in workflows {
            let safe = sanitize_filename(&wf.name);
            s.push_str(&format!(
                "- [`{}`](./workflows/{safe}.md) — {} 步\n",
                wf.name,
                wf.steps.len(),
            ));
        }
        s.push('\n');
    }
    s
}

fn render_endpoint(ep: &Endpoint, _spec: &ApiSpec) -> String {
    let mut s = String::new();
    s.push_str(&format!("# `{}`\n\n", ep.id));
    s.push_str(&format!("**`{} {}`**", ep.method.as_str(), ep.path));
    if ep.deprecated {
        s.push_str(" · ⚠️ deprecated");
    }
    s.push_str("\n\n");

    if let Some(summary) = &ep.summary {
        s.push_str(&format!("> {summary}\n\n"));
    }
    if let Some(desc) = &ep.description {
        s.push_str(&format!("{desc}\n\n"));
    }
    if !ep.tags.is_empty() {
        s.push_str(&format!("**Tags**: {}\n\n", ep.tags.join(", ")));
    }

    // 参数
    if !ep.params.is_empty() {
        s.push_str("## 参数\n\n");
        s.push_str("| 名称 | 位置 | 必填 | 类型 | 说明 |\n");
        s.push_str("|---|---|---|---|---|\n");
        for p in &ep.params {
            s.push_str(&format!(
                "| `{}` | {} | {} | `{}` | {} |\n",
                p.name,
                p.location.as_str(),
                if p.required { "✅" } else { "—" },
                p.schema.name,
                p.description.as_deref().unwrap_or(""),
            ));
        }
        s.push('\n');
    }

    // 请求体
    if let Some(rb) = &ep.request_body {
        s.push_str("## 请求体\n\n");
        s.push_str(&format!(
            "- content-type: `{}`{}\n- 必填: {}\n- schema: `{}`\n\n",
            rb.content_type,
            rb.schema
                .description
                .as_deref()
                .map(|d| format!(" — {d}"))
                .unwrap_or_default(),
            if rb.required { "✅" } else { "—" },
            rb.schema.name,
        ));
        // 渲染 resolved 树
        s.push_str(&render_resolved_schema_block(&rb.schema, 0));
    }

    // 响应
    if !ep.responses.is_empty() {
        s.push_str("## 响应\n\n");
        for r in &ep.responses {
            s.push_str(&format!(
                "- **{}**{} {}\n",
                r.status,
                r.content_type
                    .as_deref()
                    .map(|c| format!(" `{}`", c))
                    .unwrap_or_default(),
                r.description.as_deref().unwrap_or(""),
            ));
        }
        s.push('\n');
        // 把每个响应的 schema 树也渲染出来
        for r in &ep.responses {
            if let Some(schema) = &r.schema {
                if let Some(name) = Some(schema.name.as_str()).filter(|n| !n.is_empty() && *n != "any") {
                    s.push_str(&format!("\n### 响应 {} schema `{}`\n\n", r.status, name));
                    s.push_str(&render_resolved_schema_block(schema, 0));
                }
            }
        }
    }

    // agent 调用示例
    s.push_str("## Agent 调用示例\n\n");
    s.push_str("```python\n");
    s.push_str("import json, subprocess\n\n");
    s.push_str("req = {\n");
    s.push_str("    \"jsonrpc\": \"2.0\",\n");
    s.push_str("    \"id\": 1,\n");
    s.push_str("    \"method\": \"call\",\n");
    s.push_str("    \"params\": {\n");
    s.push_str(&format!("        \"endpoint_id\": {:?},\n", ep.id));
    // 给出 path_params 占位
    let path_params: Vec<String> = ep
        .params
        .iter()
        .filter(|p| matches!(p.location, ParamLocation::Path))
        .map(|p| {
            format!(
                "        {:?}: \"<{}>\",",
                p.name,
                if p.description.is_some() {
                    p.name.as_str()
                } else {
                    p.name.as_str()
                }
            )
        })
        .collect();
    if !path_params.is_empty() {
        s.push_str("        \"path_params\": {\n");
        s.push_str(&path_params.join("\n"));
        s.push_str("\n        },\n");
    } else {
        s.push_str("        \"path_params\": {},\n");
    }
    // query 占位
    let query_params: Vec<String> = ep
        .params
        .iter()
        .filter(|p| matches!(p.location, ParamLocation::Query))
        .map(|p| format!("        {:?}: \"<{}>\"", p.name, p.name))
        .collect();
    if !query_params.is_empty() {
        s.push_str("        \"query\": {\n");
        s.push_str(&query_params.join(",\n"));
        s.push_str("\n        },\n");
    } else {
        s.push_str("        \"query\": {},\n");
    }
    s.push_str("        \"headers\": {},\n");
    if ep.request_body.is_some() {
        s.push_str("        \"body\": {}\n");
    } else {
        s.push_str("        \"body\": None\n");
    }
    s.push_str("    },\n");
    s.push_str("}\n\n");
    s.push_str("proc = subprocess.run(\n");
    s.push_str("    [\"x\", \"serve\"],\n");
    s.push_str("    input=json.dumps(req),\n");
    s.push_str("    capture_output=True,\n");
    s.push_str("    text=True,\n");
    s.push_str(")\n");
    s.push_str("resp = json.loads(proc.stdout.strip())\n");
    s.push_str("```\n");
    s
}

fn sanitize_filename(s: &str) -> String {
    s.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}

/// 渲染 ResolvedSchema 树（B 阶段）
///
/// 输出 markdown 表格，递归展开 properties。遇到 `recursive: true` 终止。
/// `depth` 限制嵌套深度（防止误用导致的极深）。
fn render_resolved_schema_block(schema: &SchemaRef, depth: usize) -> String {
    if depth >= 4 {
        return format!("> 嵌套过深（>4 层），已截断\n");
    }
    let Some(resolved) = &schema.resolved else {
        // 没有 resolved 树：返回原 schema 描述
        return format!(
            "> schema `{}`（未解析）\n",
            schema.name
        );
    };
    if resolved.recursive {
        return format!("> schema `{}` — 循环引用回填\n", schema.name);
    }
    let mut out = String::new();
    render_resolved_into(&mut out, schema, resolved, depth);
    out
}

fn render_resolved_into(
    out: &mut String,
    schema: &SchemaRef,
    resolved: &ResolvedSchema,
    depth: usize,
) {
    use x_cli_core::ir::SchemaKind;
    match resolved.kind {
        SchemaKind::Object => {
            if let Some(desc) = &schema.description {
                out.push_str(&format!("> {desc}\n\n"));
            }
            out.push_str("| 字段 | 类型 | 必填 | 说明 |\n");
            out.push_str("|---|---|---|---|\n");
            for (pname, pschema) in &resolved.properties {
                let required = if resolved.required.contains(pname) {
                    "✅"
                } else {
                    "—"
                };
                let desc = pschema.description.as_deref().unwrap_or("");
                let type_label = schema_type_label(pschema);
                out.push_str(&format!("| `{pname}` | {type_label} | {required} | {desc} |\n"));
            }
            out.push('\n');
            // 递归：如果某个 property 是 object/array 且有 properties/items，再展开
            for (pname, pschema) in &resolved.properties {
                if let Some(inner) = &pschema.resolved {
                    if matches!(inner.kind, SchemaKind::Object) && !inner.properties.is_empty() {
                        out.push_str(&format!(
                            "### `{pname}` 类型（{}）\n\n",
                            pschema.name
                        ));
                        render_resolved_into(out, pschema, inner, depth + 1);
                    } else if matches!(inner.kind, SchemaKind::Array) {
                        if let Some(items) = &inner.items {
                            if let Some(items_resolved) = &items.resolved {
                                if matches!(items_resolved.kind, SchemaKind::Object)
                                    && !items_resolved.properties.is_empty()
                                {
                                    out.push_str(&format!(
                                        "### `{pname}` 数组元素（{}）\n\n",
                                        items.name
                                    ));
                                    render_resolved_into(out, items, items_resolved, depth + 1);
                                }
                            }
                        }
                    }
                }
            }
        }
        SchemaKind::Array => {
            if let Some(items) = &resolved.items {
                out.push_str(&format!("数组元素类型：`{}`\n\n", items.name));
                if let Some(items_resolved) = &items.resolved {
                    if matches!(items_resolved.kind, SchemaKind::Object)
                        && !items_resolved.properties.is_empty()
                    {
                        render_resolved_into(out, items, items_resolved, depth + 1);
                    }
                }
            }
        }
        SchemaKind::Scalar => {
            out.push_str(&format!("scalar `{}`\n\n", schema.name));
        }
        SchemaKind::Any => {
            out.push_str(&format!("any\n\n"));
        }
    }
}

fn schema_type_label(schema: &SchemaRef) -> String {
    use x_cli_core::ir::SchemaKind;
    let Some(r) = &schema.resolved else {
        return format!("`{}`", schema.name);
    };
    match r.kind {
        SchemaKind::Object => format!("object `{}`", schema.name),
        SchemaKind::Array => {
            if let Some(items) = &r.items {
                format!("array<`{}`>", items.name)
            } else {
                "array".to_string()
            }
        }
        SchemaKind::Scalar => format!("`{}`", schema.name),
        SchemaKind::Any => "any".to_string(),
    }
}

// ─────────────── Workflow 渲染（C 阶段） ───────────────

fn render_workflow(wf: &Workflow, spec: &ApiSpec) -> String {
    let mut s = String::new();
    s.push_str(&format!("# `{}`（工作流）\n\n", wf.name));
    if let Some(desc) = &wf.description {
        s.push_str(&format!("{desc}\n\n"));
    }

    // inputs
    if !wf.inputs.is_empty() {
        s.push_str("## 输入参数\n\n");
        s.push_str("| 名称 | 类型 | 必填 | 默认 | 说明 |\n");
        s.push_str("|---|---|---|---|---|\n");
        for input in &wf.inputs {
            let required = if input.default.is_none() { "✅" } else { "—" };
            let default = input
                .default
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "—".to_string());
            let desc = input.description.as_deref().unwrap_or("");
            s.push_str(&format!(
                "| `{}` | `{}` | {} | {} | {} |\n",
                input.name, input.r#type, required, default, desc
            ));
        }
        s.push('\n');
    }

    // 步骤
    s.push_str(&format!("## 步骤（共 {} 步）\n\n", wf.steps.len()));
    for (i, step) in wf.steps.iter().enumerate() {
        s.push_str(&format!("### {}. `{}`\n\n", i + 1, step.name));
        if let Some(desc) = &step.description {
            s.push_str(&format!("> {desc}\n\n"));
        }
        s.push_str(&format!("- endpoint: [`{}`](../endpoints/{}.md)\n",
            step.endpoint, sanitize_filename(&step.endpoint)));
        // inputs 解析展示
        render_step_inputs(&mut s, &step.inputs);
        s.push('\n');
    }

    // agent 调用示例
    s.push_str("## Agent 调用示例\n\n");
    s.push_str("按步骤顺序执行，每步用上一步响应填下步 inputs。\n\n");
    s.push_str("```python\n");
    s.push_str("import json, subprocess\n\n");
    s.push_str("def call(endpoint_id, path_params=None, query=None, headers=None, body=None):\n");
    s.push_str("    req = {\n");
    s.push_str("        \"jsonrpc\": \"2.0\", \"id\": 1, \"method\": \"call\",\n");
    s.push_str("        \"params\": {\n");
    s.push_str("            \"endpoint_id\": endpoint_id,\n");
    s.push_str("            \"path_params\": path_params or {},\n");
    s.push_str("            \"query\": query or {},\n");
    s.push_str("            \"headers\": headers or {},\n");
    s.push_str("            \"body\": body,\n");
    s.push_str("        },\n");
    s.push_str("    }\n");
    s.push_str("    proc = subprocess.run(\n");
    s.push_str("        [\"x\", \"serve\"],\n");
    s.push_str("        f\"--skill {SKILL_DIR}\".split(),\n");
    s.push_str("        input=json.dumps(req), capture_output=True, text=True,\n");
    s.push_str("    )\n");
    s.push_str("    return json.loads(proc.stdout.strip())[\"result\"]\n\n");
    s.push_str("SKILL_DIR = \"./this-skill\"  # 改成本地 skill 目录\n\n");
    // 给每个 step 写示例
    for (i, step) in wf.steps.iter().enumerate() {
        s.push_str(&format!("# Step {}: {}\n", i + 1, step.name));
        s.push_str(&format!("resp_{} = call({:?}", step.name, step.endpoint));
        // 给个 path_params 示例（不实际解析 $input，按字符串直接传）
        if !step.inputs.path_params.is_empty() {
            s.push_str(", path_params={");
            for (k, v) in &step.inputs.path_params {
                s.push_str(&format!("{:?}: {:?}, ", k, v));
            }
            s.push('}');
        }
        if !step.inputs.body.is_empty() {
            s.push_str(", body={");
            for (k, v) in &step.inputs.body {
                s.push_str(&format!("{:?}: {:?}, ", k, v));
            }
            s.push('}');
        }
        s.push_str(")\n");
        if i < wf.steps.len() - 1 {
            s.push('\n');
        }
    }
    s.push_str("```\n");

    // 隐含要求：endpoint 必须存在于 spec（用注释提示）
    for step in &wf.steps {
        if !spec.endpoints.contains_key(&step.endpoint) {
            s.push_str(&format!(
                "\n> ⚠️ 警告：step `{}` 引用的 endpoint `{}` 不在当前 OpenAPI 文档里。\n",
                step.name, step.endpoint
            ));
        }
    }

    s
}

fn render_step_inputs(s: &mut String, inputs: &x_cli_core::ir::StepInputs) {
    let has_any = !inputs.path_params.is_empty()
        || !inputs.query.is_empty()
        || !inputs.headers.is_empty()
        || !inputs.body.is_empty();
    if !has_any {
        return;
    }
    s.push_str("- inputs:\n");
    if !inputs.path_params.is_empty() {
        s.push_str("  - path_params:\n");
        for (k, v) in &inputs.path_params {
            s.push_str(&format!("    - `{}` ← {}\n", k, InputRef::parse(v).describe()));
        }
    }
    if !inputs.query.is_empty() {
        s.push_str("  - query:\n");
        for (k, v) in &inputs.query {
            s.push_str(&format!("    - `{}` ← {}\n", k, InputRef::parse(v).describe()));
        }
    }
    if !inputs.headers.is_empty() {
        s.push_str("  - headers:\n");
        for (k, v) in &inputs.headers {
            s.push_str(&format!("    - `{}` ← {}\n", k, InputRef::parse(v).describe()));
        }
    }
    if !inputs.body.is_empty() {
        s.push_str("  - body:\n");
        for (k, v) in &inputs.body {
            s.push_str(&format!("    - `{}` ← {}\n", k, InputRef::parse(v).describe()));
        }
    }
}

trait MethodStr {
    fn as_str(&self) -> &'static str;
}

impl MethodStr for HttpMethod {
    fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
        }
    }
}

trait LocStr {
    fn as_str(&self) -> &'static str;
}

impl LocStr for ParamLocation {
    fn as_str(&self) -> &'static str {
        match self {
            ParamLocation::Path => "path",
            ParamLocation::Query => "query",
            ParamLocation::Header => "header",
            ParamLocation::Cookie => "cookie",
        }
    }
}
