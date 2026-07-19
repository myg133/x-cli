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
use serde_json::json;
use std::fs;
use std::path::Path;
use x_cli_core::ir::{ApiSpec, Endpoint, HttpMethod, InputRef, ParamLocation, ResolvedSchema, Response, SchemaRef, Workflow, WorkflowStep};

/// Skill 输出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillFormat {
    /// 默认：每个 endpoint / workflow 一份 markdown + SKILL.md 索引（适合人读、agent 参考）
    #[default]
    Markdown,
    /// Anthropic 风格：单 SKILL.md 含 frontmatter（描述何时用），不带分文件
    Anthropic,
    /// OpenAI function calling：单个 functions.json，含 tools 数组
    OpenAITools,
}

/// SkillEmitter trait — 不同平台各自实现
#[async_trait]
pub trait SkillEmitter {
    /// 把 IR 渲染到 out_dir
    async fn emit(
        &self,
        spec: &ApiSpec,
        workflows: &[Workflow],
        out_dir: &Path,
        format: SkillFormat,
    ) -> Result<()>;
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
    async fn emit(
        &self,
        spec: &ApiSpec,
        workflows: &[Workflow],
        out_dir: &Path,
        format: SkillFormat,
    ) -> Result<()> {
        fs::create_dir_all(out_dir).context("create out_dir")?;

        match format {
            SkillFormat::Markdown => self.emit_markdown(spec, workflows, out_dir).await,
            SkillFormat::Anthropic => self.emit_anthropic(spec, workflows, out_dir).await,
            SkillFormat::OpenAITools => self.emit_openai(spec, workflows, out_dir).await,
        }
    }
}

impl MarkdownEmitter {
    /// markdown 模式（默认）：SKILL.md + endpoints/*.md + workflows/*.md + workflows/*.yaml
    async fn emit_markdown(
        &self,
        spec: &ApiSpec,
        workflows: &[Workflow],
        out_dir: &Path,
    ) -> Result<()> {
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
                // 机器可读版本：runtime 用这个加载
                let yaml = serde_yaml::to_string(wf).context("serialize workflow yaml")?;
                fs::write(
                    out_dir.join("workflows").join(format!("{safe}.yaml")),
                    yaml,
                )
                .context("write workflow yaml")?;
            }
        }

        Ok(())
    }

    /// Anthropic 模式：单个 SKILL.md 含 YAML frontmatter
    async fn emit_anthropic(
        &self,
        spec: &ApiSpec,
        workflows: &[Workflow],
        out_dir: &Path,
    ) -> Result<()> {
        let skill = render_anthropic_skill(spec, workflows);
        fs::write(out_dir.join("SKILL.md"), skill).context("write SKILL.md")?;
        Ok(())
    }

    /// OpenAI function calling 模式：单个 functions.json
    async fn emit_openai(
        &self,
        spec: &ApiSpec,
        workflows: &[Workflow],
        out_dir: &Path,
    ) -> Result<()> {
        let tools = render_openai_tools(spec, workflows);
        fs::write(
            out_dir.join("functions.json"),
            serde_json::to_string_pretty(&tools).context("serialize tools")?,
        )
        .context("write functions.json")?;
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
                    safe = url_encode_path(&sanitize_filename(id)),
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
            let safe = url_encode_path(&sanitize_filename(&wf.name));
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
        // 把响应按 schema signature 分组，相同 signature 的状态码合并
        // signature = (content_type, schema_name, schema_json) 元组的字符串形式
        let mut groups: Vec<(String, Vec<&Response>)> = Vec::new();
        for r in &ep.responses {
            let sig = response_signature(r);
            if let Some(group) = groups.iter_mut().find(|(s, _)| s == &sig) {
                group.1.push(r);
            } else {
                groups.push((sig, vec![r]));
            }
        }

        // 第一段：状态码行（合并同 signature 的）
        for (_, rs) in &groups {
            let statuses = merge_statuses(rs);
            // 用第一个 response 的描述（同 signature 描述应该一样）
            let first = rs[0];
            s.push_str(&format!(
                "- **{statuses}**{} {}\n",
                first
                    .content_type
                    .as_deref()
                    .map(|c| format!(" `{}`", c))
                    .unwrap_or_default(),
                first.description.as_deref().unwrap_or(""),
            ));
        }
        s.push('\n');

        // 第二段：每个 signature 一份 schema 树
        for (_, rs) in &groups {
            let first = rs[0];
            if let Some(schema) = &first.schema {
                if !schema.name.is_empty() && schema.name != "any" {
                    let statuses = merge_statuses(rs);
                    s.push_str(&format!("\n### 响应 {statuses} schema `{}`\n\n", schema.name));
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

/// 路径用 sanitize：空格 → '-', 文件系统不安全字符 → '_'。
/// 区别于 sanitize_filename：路径里 '-' 比 '_' 视觉更友好，
/// 但 '-' 不能作为文件名起始（在 shell 命令里可能误判为 flag），所以文件仍用 _。
fn sanitize_path(s: &str) -> String {
    s.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
        .replace(' ', "-")
}

/// URL 编码一段路径，保留 `/` 不变（路径分隔符）。
/// 用于 markdown 链接里把含空格的 endpoint / domain 名字转成 URL-safe 形式，
/// 让各种 agent 平台都能正确跳转。
fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '/') {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// 计算响应的"schema signature"：相同 signature 的响应会被合并显示。
/// 当前用 (content_type, schema_name, schema_json_str) 三元组。
fn response_signature(r: &Response) -> String {
    let ct = r.content_type.as_deref().unwrap_or("");
    let (name, json) = match &r.schema {
        Some(s) => (
            s.name.as_str(),
            serde_json::to_string(&s.json_schema).unwrap_or_default(),
        ),
        None => ("", String::new()),
    };
    format!("{ct}|{name}|{json}")
}

/// 把一组状态码合并成逗号分隔的字符串。连续状态码用 `-` 简写。
/// 例：[200, 201, 400, 401, 500] → "200, 201, 400, 401, 500"
///     [200, 201, 202] → "200-202"
fn merge_statuses(rs: &[&Response]) -> String {
    let mut codes: Vec<u16> = rs.iter().map(|r| r.status).collect();
    codes.sort();
    codes.dedup();
    // 合并连续区间
    let mut out = String::new();
    let mut i = 0;
    while i < codes.len() {
        let start = codes[i];
        let mut end = start;
        while i + 1 < codes.len() && codes[i + 1] == end + 1 {
            i += 1;
            end = codes[i];
        }
        if !out.is_empty() {
            out.push_str(", ");
        }
        if start == end {
            out.push_str(&start.to_string());
        } else if end - start == 1 {
            out.push_str(&format!("{start}, {end}"));
        } else {
            out.push_str(&format!("{start}-{end}"));
        }
        i += 1;
    }
    out
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

// ─────────────── Anthropic skill 渲染（E 阶段） ───────────────

/// Anthropic SKILL.md：YAML frontmatter + 完整 API 描述（合并到一个文件）
fn render_anthropic_skill(spec: &ApiSpec, workflows: &[Workflow]) -> String {
    let mut s = String::new();
    s.push_str("---\n");
    s.push_str(&format!("name: {}\n", sanitize_for_frontmatter(&spec.title)));
    s.push_str(&format!("description: {}\n", build_anthropic_description(spec, workflows)));
    s.push_str("---\n\n");
    s.push_str(&format!("# {} — x-cli skill\n\n", spec.title));
    if let Some(desc) = &spec.description {
        s.push_str(&format!("{desc}\n\n"));
    }
    if let Some(url) = &spec.base_url {
        s.push_str(&format!("**Base URL**: `{url}`\n\n"));
    }

    // 调用约定
    s.push_str("## 调用约定\n\n");
    s.push_str("通过 JSON-RPC 2.0 over stdio 调用：\n\n");
    s.push_str("```\n");
    s.push_str("\"jsonrpc\":\"2.0\", \"id\":1, \"method\":\"call\",\n");
    s.push_str(" \"params\":{\"endpoint_id\":\"<id>\", \"path_params\":{}, \"query\":{}, \"headers\":{}, \"body\":{}}\n");
    s.push_str("```\n\n");
    s.push_str("工作流用 `workflow.run` method：\n\n");
    s.push_str("```\n");
    s.push_str("\"method\":\"workflow.run\", \"params\":{\"workflow\":\"<name>\", \"inputs\":{...}}\n");
    s.push_str("```\n\n");

    // 业务域
    s.push_str("## 业务域\n\n");
    for d in &spec.domains {
        s.push_str(&format!("### {}\n\n", d.name));
        for id in &d.endpoint_ids {
            if let Some(ep) = spec.endpoints.get(id) {
                let summary = ep
                    .summary
                    .as_deref()
                    .map(|x| format!(" — {x}"))
                    .unwrap_or_default();
                s.push_str(&format!(
                    "- `{} {}` — {}{}\n",
                    ep.method.as_str(),
                    ep.path,
                    id,
                    summary,
                ));
            }
        }
        s.push('\n');
    }

    // 工作流
    if !workflows.is_empty() {
        s.push_str("## 工作流\n\n");
        for wf in workflows {
            s.push_str(&format!("### {}\n\n", wf.name));
            if let Some(desc) = &wf.description {
                s.push_str(&format!("{desc}\n\n"));
            }
            s.push_str(&format!("- 步数: {}\n", wf.steps.len()));
            s.push_str(&format!("- 外部 inputs: {}\n", wf.inputs.len()));
            s.push_str("\n调用示例：\n\n```\n");
            s.push_str(&format!(
                "x serve --skill <dir>  # 启动后：\n{{\"method\":\"workflow.run\",\"params\":{{\"workflow\":\"{}\",\"inputs\":{{...}}}}}}\n",
                wf.name
            ));
            s.push_str("```\n\n");
        }
    }

    s
}

/// 构造 Anthropic 的 description 字段（让 Claude 知道何时加载这个 skill）
fn build_anthropic_description(spec: &ApiSpec, workflows: &[Workflow]) -> String {
    let domain_names: Vec<&str> = spec.domains.iter().take(8).map(|d| d.name.as_str()).collect();
    let domain_phrase = if domain_names.is_empty() {
        "通用 API".to_string()
    } else {
        format!("覆盖 {} 等业务域", domain_names.join("、"))
    };
    let workflow_phrase = if workflows.is_empty() {
        String::new()
    } else {
        format!(" 包含 {} 个工作流（多步任务）。", workflows.len())
    };
    let version = spec.version.trim_start_matches('v');
    format!(
        "API 版本 {}，{} 个接口，{}。{}当用户问及这些业务时使用此 skill。",
        version,
        spec.endpoints.len(),
        domain_phrase,
        workflow_phrase,
    )
}

/// frontmatter 值要简单（不能含 :、换行、引号）
fn sanitize_for_frontmatter(s: &str) -> String {
    s.replace(['\n', '\r', ':', '"'], " ")
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '（' | '）' | '、'))
        .collect()
}

// ─────────────── OpenAI function calling 渲染（E 阶段） ───────────────

/// OpenAI function calling JSON：{ "tools": [ { "type": "function", "function": {...} } ] }
fn render_openai_tools(spec: &ApiSpec, workflows: &[Workflow]) -> serde_json::Value {
    let mut tools = Vec::new();

    for ep in spec.endpoints.values() {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": ep.id,
                "description": build_endpoint_description(ep),
                "parameters": build_endpoint_parameters(ep),
            }
        }));
    }

    for wf in workflows {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": format!("workflow.{}", wf.name),
                "description": wf.description.clone().unwrap_or_else(|| wf.name.clone()),
                "parameters": build_workflow_parameters(wf),
            }
        }));
    }

    json!({ "tools": tools })
}

fn build_endpoint_description(ep: &Endpoint) -> String {
    let summary = ep.summary.as_deref().unwrap_or("");
    let description = ep.description.as_deref().unwrap_or("");
    let method_path = format!("{} {}", ep.method.as_str(), ep.path);
    if description.is_empty() {
        if summary.is_empty() {
            method_path
        } else {
            format!("{method_path} — {summary}")
        }
    } else {
        format!("{method_path} — {summary}\n{description}")
    }
}

fn build_endpoint_parameters(ep: &Endpoint) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    // path / query / header 合并为 properties（因为 function calling 不区分位置）
    for p in &ep.params {
        properties.insert(
            p.name.clone(),
            json!({
                "type": schema_json_type(p.schema.name.as_str()),
                "description": p.description.clone().unwrap_or_default(),
            }),
        );
        if p.required {
            required.push(p.name.clone());
        }
    }

    // body 作为 body 参数
    if let Some(rb) = &ep.request_body {
        properties.insert(
            "body".to_string(),
            json!({
                "type": "object",
                "description": rb.schema.name.clone(),
            }),
        );
        if rb.required {
            required.push("body".to_string());
        }
    }

    let required = if required.is_empty() {
        None
    } else {
        Some(required)
    };

    json!({
        "type": "object",
        "properties": properties,
        "required": required.unwrap_or_default(),
    })
}

fn build_workflow_parameters(wf: &Workflow) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for input in &wf.inputs {
        properties.insert(
            input.name.clone(),
            json!({
                "type": schema_json_type(&input.r#type),
                "description": input.description.clone().unwrap_or_default(),
            }),
        );
        if input.default.is_none() {
            required.push(input.name.clone());
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn schema_json_type(name: &str) -> &'static str {
    match name {
        "string" => "string",
        "integer" => "integer",
        "number" => "number",
        "boolean" => "boolean",
        "array" => "array",
        "object" => "object",
        _ => "string",
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
            step.endpoint, url_encode_path(&sanitize_filename(&step.endpoint))));
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
