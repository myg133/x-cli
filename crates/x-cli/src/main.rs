//! x-cli 主入口
//!
//! 子命令：
//! - `x parse <openapi>`         解析并打印 IR（debug）
//! - `x emit <openapi> --out DIR` 生成 markdown skill 到 DIR
//! - `x serve --skill DIR`       启动 stdio JSON-RPC 服务（agent 调 x 的入口）

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use x_cli_core::ir::ApiSpec;
use x_cli_core::parse_openapi;
use x_cli_emitter_md::{MarkdownEmitter, SkillEmitter};
use x_cli_runtime::{serve_stdio, AuthProfile, HttpCaller};

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
    },
    /// 启动 stdio JSON-RPC 服务（agent 加载 skill 后调这个）
    Serve {
        /// skill 目录（含 .x-cli/ir.json）
        #[arg(short, long)]
        skill: PathBuf,
        /// 覆盖 base URL（默认用 IR 里的）
        #[arg(long)]
        base_url: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Parse { openapi } => cmd_parse(openapi),
        Cmd::Emit { openapi, out } => cmd_emit(openapi, out).await,
        Cmd::Serve { skill, base_url } => cmd_serve(skill, base_url).await,
    }
}

fn cmd_parse(openapi: PathBuf) -> Result<()> {
    let spec = parse_openapi(&openapi).context("parse openapi")?;
    println!("{}", serde_json::to_string_pretty(&spec)?);
    Ok(())
}

async fn cmd_emit(openapi: PathBuf, out: PathBuf) -> Result<()> {
    let spec = parse_openapi(&openapi).context("parse openapi")?;
    std::fs::create_dir_all(&out).context("create out dir")?;
    let emitter = MarkdownEmitter::new();
    emitter
        .emit(&spec, &out)
        .await
        .context("emit markdown")?;
    // 缓存 IR 供 serve 使用
    let cache_dir = out.join(".x-cli");
    std::fs::create_dir_all(&cache_dir).context("create cache dir")?;
    let ir_json = serde_json::to_string_pretty(&spec)?;
    std::fs::write(cache_dir.join("ir.json"), ir_json).context("write ir.json")?;
    println!(
        "✓ 解析 {} 个接口，写入 {}\n  业务域: {}",
        spec.endpoints.len(),
        out.display(),
        spec.domains.len()
    );
    Ok(())
}

async fn cmd_serve(skill: PathBuf, base_url_override: Option<String>) -> Result<()> {
    let ir_path = skill.join(".x-cli").join("ir.json");
    let raw = std::fs::read_to_string(&ir_path)
        .with_context(|| format!("read {}", ir_path.display()))?;
    let spec: ApiSpec = serde_json::from_str(&raw).context("parse ir.json")?;
    let base_url = base_url_override.or(spec.base_url.clone());
    let caller = HttpCaller::new(AuthProfile::default()).context("build http caller")?;
    serve_stdio(Arc::new(spec), base_url, caller).await;
    Ok(())
}
