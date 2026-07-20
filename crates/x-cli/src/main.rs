//! x-cli 主入口
//!
//! 子命令：
//! - `x parse <openapi>`         解析并打印 IR（debug）
//! - `x emit <openapi> --out DIR` 生成 markdown skill 到 DIR
//! - `x serve --skill DIR`       启动 stdio JSON-RPC 服务（agent 调 x 的入口）

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use x_cli_core::ir::{ApiSpec, Workflow};
use x_cli_core::{parse_openapi, parse_workflow};
use x_cli_emitter_md::{MarkdownEmitter, SkillEmitter, SkillFormat};
use x_cli_runtime::{build_auth_profile, serve_stdio, AuthProfile, HttpCaller};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum SkillFormatArg {
    Markdown,
    Anthropic,
    Openai,
}

impl From<SkillFormatArg> for SkillFormat {
    fn from(a: SkillFormatArg) -> Self {
        match a {
            SkillFormatArg::Markdown => SkillFormat::Markdown,
            SkillFormatArg::Anthropic => SkillFormat::Anthropic,
            SkillFormatArg::Openai => SkillFormat::OpenAITools,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "x", version, about = "把后端 OpenAPI 转成 agent 可用的 skill")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// 解析 OpenAPI 并打印 IR（debug 用）
    Parse {
        /// OpenAPI 文件路径（yaml/json）
        openapi: PathBuf,
    },
    /// 解析 OpenAPI 并生成 markdown skill 到目录
    Emit {
        /// OpenAPI 文件路径
        openapi: PathBuf,
        /// 输出目录
        #[arg(short, long)]
        out: PathBuf,
        /// 可选：workflow.yaml 路径
        #[arg(long)]
        workflow: Vec<PathBuf>,
        /// 输出格式
        #[arg(long, value_enum, default_value_t = SkillFormatArg::Markdown)]
        format: SkillFormatArg,
    },
    /// 启动 stdio JSON-RPC 服务（agent 加载 skill 后调这个）
    Serve {
        /// skill 目录（含 .x-cli/ir.json）
        #[arg(short, long)]
        skill: PathBuf,
        /// 覆盖 base URL（默认用 IR 里的）
        #[arg(long)]
        base_url: Option<String>,
        /// Bearer token：自动加 Authorization: Bearer <TOKEN>
        #[arg(long, value_name = "TOKEN")]
        auth_bearer: Vec<String>,
        /// 自定义请求头：KEY=VALUE 格式，可多次
        /// 例：--auth-header "X-API-Key=xxx" --auth-header "X-Tenant=acme"
        #[arg(long, value_name = "KEY=VALUE")]
        auth_header: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Parse { openapi } => cmd_parse(openapi),
        Cmd::Emit {
            openapi,
            out,
            workflow,
            format,
        } => cmd_emit(openapi, out, workflow, format.into()).await,
        Cmd::Serve {
            skill,
            base_url,
            auth_bearer,
            auth_header,
        } => cmd_serve(skill, base_url, auth_bearer, auth_header).await,
    }
}

fn cmd_parse(openapi: PathBuf) -> Result<()> {
    let spec = parse_openapi(&openapi).context("parse openapi")?;
    println!("{}", serde_json::to_string_pretty(&spec)?);
    Ok(())
}

async fn cmd_emit(
    openapi: PathBuf,
    out: PathBuf,
    workflows: Vec<PathBuf>,
    format: SkillFormat,
) -> Result<()> {
    let spec = parse_openapi(&openapi).context("parse openapi")?;
    std::fs::create_dir_all(&out).context("create out dir")?;
    let emitter = MarkdownEmitter::new();

    // 解析所有 workflow（C 阶段）
    let mut parsed_workflows = Vec::new();
    for wf_path in &workflows {
        let wf = parse_workflow(wf_path)
            .with_context(|| format!("parse workflow {}", wf_path.display()))?;
        // 校验 endpoint 引用
        for step in &wf.steps {
            if !spec.endpoints.contains_key(&step.endpoint) {
                anyhow::bail!(
                    "workflow `{}` 引用了不存在的 endpoint `{}`",
                    wf.name,
                    step.endpoint
                );
            }
        }
        parsed_workflows.push(wf);
    }

    emitter
        .emit(&spec, &parsed_workflows, &out, format)
        .await
        .context("emit")?;

    // 缓存 IR 供 serve 使用（任何 format 都要，因为 serve 跑 workflow 时需要）
    let cache_dir = out.join(".x-cli");
    std::fs::create_dir_all(&cache_dir).context("create cache dir")?;
    let ir_json = serde_json::to_string_pretty(&spec)?;
    std::fs::write(cache_dir.join("ir.json"), ir_json).context("write ir.json")?;
    println!(
        "✓ 解析 {} 个接口、{} 个工作流，格式 {} 写入 {}",
        spec.endpoints.len(),
        parsed_workflows.len(),
        format_label(format),
        out.display()
    );
    Ok(())
}

fn format_label(f: SkillFormat) -> &'static str {
    match f {
        SkillFormat::Markdown => "markdown",
        SkillFormat::Anthropic => "anthropic",
        SkillFormat::OpenAITools => "openai-tools",
    }
}

async fn cmd_serve(
    skill: PathBuf,
    base_url_override: Option<String>,
    auth_bearer: Vec<String>,
    auth_header: Vec<String>,
) -> Result<()> {
    let ir_path = skill.join(".x-cli").join("ir.json");
    let raw =
        std::fs::read_to_string(&ir_path).with_context(|| format!("read {}", ir_path.display()))?;
    let spec: ApiSpec = serde_json::from_str(&raw).context("parse ir.json")?;

    // 加载 workflows/ 下的所有 .yaml
    let workflows = load_workflows(&skill).context("load workflows")?;
    if !workflows.is_empty() {
        println!("✓ 加载 {} 个工作流", workflows.len());
    }

    let base_url = base_url_override.or(spec.base_url.clone());
    let auth = build_auth_profile(&auth_bearer, &auth_header)?;
    let caller = HttpCaller::new(auth).context("build http caller")?;
    if !auth_bearer.is_empty() || !auth_header.is_empty() {
        println!(
            "✓ 注入 {} 个认证 header",
            auth_bearer.len() + auth_header.len()
        );
    }
    serve_stdio(Arc::new(spec), workflows, base_url, caller).await;
    Ok(())
}

fn load_workflows(skill_dir: &std::path::Path) -> Result<BTreeMap<String, Arc<Workflow>>> {
    let mut out = BTreeMap::new();
    let wf_dir = skill_dir.join("workflows");
    if !wf_dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&wf_dir).context("read workflows dir")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let wf: Workflow =
            parse_workflow(&path).with_context(|| format!("parse {}", path.display()))?;
        out.insert(wf.name.clone(), Arc::new(wf));
    }
    Ok(out)
}
