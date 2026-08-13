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
use x_cli_core::ir::{ApiSpec, CliSpec, Workflow};
use x_cli_core::parse_auth_config_str;
use x_cli_core::{parse_cli_spec, parse_openapi, parse_workflow};
use x_cli_emitter_mcp::McpEmitter;
use x_cli_emitter_md::{MarkdownEmitter, SkillEmitter, SkillFormat};
use x_cli_runtime::{serve_mcp_stdio, serve_stdio, HttpCaller, Session};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum SkillFormatArg {
    Markdown,
    Anthropic,
    Openai,
    Mcp,
}

impl From<SkillFormatArg> for SkillFormat {
    fn from(a: SkillFormatArg) -> Self {
        match a {
            SkillFormatArg::Markdown => SkillFormat::Markdown,
            SkillFormatArg::Anthropic => SkillFormat::Anthropic,
            SkillFormatArg::Openai => SkillFormat::OpenAITools,
            SkillFormatArg::Mcp => {
                unreachable!("MCP format 不走原有的 SkillEmitter trait")
            }
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
    /// 解析并生成 skill（支持 markdown / anthropic / openai / mcp）
    Emit {
        /// OpenAPI 文件路径
        openapi: PathBuf,
        /// 输出目录
        #[arg(short, long)]
        out: PathBuf,
        /// 可选：workflow.yaml 路径
        #[arg(long)]
        workflow: Vec<PathBuf>,
        /// CLI 工具定义文件（yaml，agent 按 CliSpec schema 写）
        #[arg(long)]
        cli_tools: Option<PathBuf>,
        /// 输出格式
        #[arg(long, value_enum, default_value_t = SkillFormatArg::Markdown)]
        format: SkillFormatArg,
    },
    /// 启动 stdio JSON-RPC / MCP 服务
    Serve {
        /// skill 目录（含 .x-cli/ir.json）
        #[arg(short, long)]
        skill: PathBuf,
        /// 使用 MCP 协议（而非自定义 JSON-RPC）
        #[arg(long)]
        mcp: bool,
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
            cli_tools,
            format,
        } => cmd_emit(openapi, out, workflow, cli_tools, format).await,
        Cmd::Serve {
            skill,
            mcp,
            base_url,
            auth_bearer,
            auth_header,
        } => cmd_serve(skill, mcp, base_url, auth_bearer, auth_header).await,
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
    cli_tools: Option<PathBuf>,
    format: SkillFormatArg,
) -> Result<()> {
    let spec = parse_openapi(&openapi).context("parse openapi")?;
    std::fs::create_dir_all(&out).context("create out dir")?;

    // 解析所有 workflow
    let mut parsed_workflows = Vec::new();
    for wf_path in &workflows {
        let wf = parse_workflow(wf_path)
            .with_context(|| format!("parse workflow {}", wf_path.display()))?;
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

    // 解析 CLI 工具（如果提供了）
    let cli_spec = if let Some(ref ct_path) = cli_tools {
        Some(
            parse_cli_spec(ct_path)
                .with_context(|| format!("parse cli-tools {}", ct_path.display()))?,
        )
    } else {
        None
    };

    if format == SkillFormatArg::Mcp {
        // MCP 格式：直接用 McpEmitter
        McpEmitter::emit_mcp(&spec, &parsed_workflows, cli_spec.as_ref(), &out)
            .context("emit mcp")?;
    } else {
        // 其他格式：走现有的 SkillEmitter trait
        let emitter = MarkdownEmitter::new();
        emitter
            .emit(&spec, &parsed_workflows, &out, format.into())
            .await
            .context("emit")?;
    }

    // 缓存 IR 供 serve 使用
    let cache_dir = out.join(".x-cli");
    std::fs::create_dir_all(&cache_dir).context("create cache dir")?;
    let ir_json = serde_json::to_string_pretty(&spec)?;
    std::fs::write(cache_dir.join("ir.json"), ir_json).context("write ir.json")?;

    // 如果有 CliSpec，也缓存一份
    if let Some(ref cs) = cli_spec {
        let cli_json = serde_json::to_string_pretty(cs)?;
        std::fs::write(cache_dir.join("cli.json"), cli_json).context("write cli.json")?;
    }

    // 确定格式标签
    let format_name = match format {
        SkillFormatArg::Mcp => "mcp",
        f => format_label(f.into()),
    };

    println!(
        "✓ 解析 {} 个接口、{} 个工作流{}，格式 {} 写入 {}",
        spec.endpoints.len(),
        parsed_workflows.len(),
        if cli_spec.is_some() {
            format!("、{} 个 CLI 工具", cli_spec.unwrap().tools.len())
        } else {
            String::new()
        },
        format_name,
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
    mcp: bool,
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

    // MCP 模式下，尝试加载 cli.json
    let cli_spec = if mcp {
        let cli_path = skill.join(".x-cli").join("cli.json");
        if cli_path.exists() {
            let raw = std::fs::read_to_string(&cli_path)
                .with_context(|| format!("read {}", cli_path.display()))?;
            let cs: CliSpec = serde_json::from_str(&raw).context("parse cli.json")?;
            println!("✓ 加载 {} 个 CLI 工具", cs.tools.len());
            Some(Arc::new(cs))
        } else {
            None
        }
    } else {
        None
    };

    let base_url = base_url_override.or(spec.base_url.clone());
    let session = build_session(&skill, base_url.as_deref(), &auth_bearer, &auth_header).await?;
    let caller = HttpCaller::new(session).context("build http caller")?;

    if mcp {
        serve_mcp_stdio(Arc::new(spec), workflows, cli_spec, base_url, caller).await;
    } else {
        serve_stdio(Arc::new(spec), workflows, base_url, caller).await;
    }
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

/// 构造 Session。优先级:`<skill>/auth.yaml` > CLI flag > 无 auth
async fn build_session(
    skill_dir: &std::path::Path,
    base_url: Option<&str>,
    auth_bearer: &[String],
    auth_header: &[String],
) -> Result<Session> {
    let auth_yaml = skill_dir.join("auth.yaml");
    if auth_yaml.exists() {
        let raw = std::fs::read_to_string(&auth_yaml)
            .with_context(|| format!("read {}", auth_yaml.display()))?;
        let cfg = parse_auth_config_str(&raw)
            .map_err(|e| anyhow::anyhow!("parse {}: {e}", auth_yaml.display()))?;
        let session = Session::from_config(cfg, base_url.map(|s| s.to_string()))
            .await
            .with_context(|| format!("initial login via {}", auth_yaml.display()))?;
        println!("✓ 从 {} 加载 session 配置", auth_yaml.display());
        Ok(session)
    } else if !auth_bearer.is_empty() || !auth_header.is_empty() {
        let session = Session::from_cli_flags(auth_bearer, auth_header)?;
        println!(
            "✓ 注入 {} 个认证 header(来自 CLI flag)",
            auth_bearer.len() + auth_header.len()
        );
        Ok(session)
    } else {
        Ok(Session::empty())
    }
}
