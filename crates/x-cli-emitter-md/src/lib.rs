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
use x_cli_core::ir::{ApiSpec, Endpoint, HttpMethod, ParamLocation};

/// SkillEmitter trait — 不同平台各自实现
#[async_trait]
pub trait SkillEmitter {
    /// 把 IR 渲染到 out_dir
    async fn emit(&self, spec: &ApiSpec, out_dir: &Path) -> Result<()>;
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
    async fn emit(&self, spec: &ApiSpec, out_dir: &Path) -> Result<()> {
        fs::create_dir_all(out_dir).context("create out_dir")?;
        fs::create_dir_all(out_dir.join("endpoints")).context("create endpoints dir")?;

        // SKILL.md 总索引
        let index = render_index(spec);
        fs::write(out_dir.join("SKILL.md"), index).context("write SKILL.md")?;

        // 每个 endpoint 一份
        for ep in spec.endpoints.values() {
            let body = render_endpoint(ep, spec);
            let safe = sanitize_filename(&ep.id);
            fs::write(out_dir.join("endpoints").join(format!("{safe}.md")), body)
                .context("write endpoint md")?;
        }

        Ok(())
    }
}

fn render_index(spec: &ApiSpec) -> String {
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
