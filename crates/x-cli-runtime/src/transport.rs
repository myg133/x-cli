//! JSON-RPC over stdio 传输层
//!
//! 一行一个 JSON 请求，一行一个 JSON 响应（带换行分隔）。
//! 这是 skill ↔ x-cli 的稳定 ABI。B 阶段可加 ndjson 批处理、Stream 响应、心跳。

use crate::http::HttpCaller;
use crate::workflow_executor::WorkflowExecutor;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tracing::{debug, error, warn};
use x_cli_core::ir::{ApiSpec, Workflow};
use x_cli_core::protocol::{
    error_code, CallParams, CallResult, RpcError, RpcId, RpcMethod, RpcRequest, RpcResponse,
    WorkflowRunParams, WorkflowRunResult,
};

/// 启动 stdio 上的 JSON-RPC 服务
pub async fn serve_stdio(
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    base_url: Option<String>,
    caller: HttpCaller,
) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    serve(stdin, stdout, spec, workflows, base_url, caller).await;
}

/// 在任意 reader/writer 上跑 JSON-RPC 服务（用于测试和未来的 sidecar 模式）
pub async fn serve<R, W>(
    reader: R,
    mut writer: W,
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    base_url: Option<String>,
    caller: HttpCaller,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(reader).lines();

    let executor = Arc::new(WorkflowExecutor::new(
        spec.clone(),
        workflows,
        base_url.clone(),
        caller.clone(),
    ));

    debug!("x-cli runtime ready (JSON-RPC)");

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let resp = handle_line(line, &spec, &executor).await;
        match resp {
            Ok(r) => {
                if let Ok(s) = serde_json::to_string(&r) {
                    let _ = writer.write_all(s.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                    let _ = writer.flush().await;
                }
            }
            Err(rpc_err) => {
                let id = parse_id(line);
                let r = RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(rpc_err),
                };
                if let Ok(s) = serde_json::to_string(&r) {
                    let _ = writer.write_all(s.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                    let _ = writer.flush().await;
                }
            }
        }
    }
    debug!("x-cli runtime exited (input closed)");
}

async fn handle_line(
    line: &str,
    spec: &ApiSpec,
    executor: &Arc<WorkflowExecutor>,
) -> Result<RpcResponse, RpcError> {
    let req: RpcRequest = serde_json::from_str(line)
        .map_err(|e| RpcError {
            code: error_code::PARSE_ERROR,
            message: format!("invalid JSON-RPC: {e}"),
            data: None,
        })?;

    match req.method {
        RpcMethod::Ping => Ok(RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(serde_json::json!({ "pong": true })),
            error: None,
        }),
        RpcMethod::Call => {
            let params: CallParams = serde_json::from_value(req.params.clone()).map_err(|e| {
                RpcError {
                    code: error_code::INVALID_PARAMS,
                    message: format!("invalid call params: {e}"),
                    data: None,
                }
            })?;
            let endpoint = spec.endpoints.get(&params.endpoint_id).ok_or_else(|| RpcError {
                code: error_code::ENDPOINT_NOT_FOUND,
                message: format!("endpoint not found: {}", params.endpoint_id),
                data: None,
            })?;
            let path_params = params.path_params;
            let query = params.query;
            let headers = params.headers;
            let body = params.body;
            match executor
                .http_caller()
                .call(
                    endpoint,
                    executor.base_url().as_deref(),
                    &path_params,
                    &query,
                    &headers,
                    body.as_ref(),
                )
                .await
            {
                Ok(r) => {
                    let result = CallResult {
                        status: r.status,
                        headers: r.headers,
                        body: r.body,
                    };
                    let value = serde_json::to_value(&result).unwrap_or(Value::Null);
                    Ok(RpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(value),
                        error: None,
                    })
                }
                Err(e) => {
                    error!(error = %e, "HTTP call failed");
                    Err(RpcError {
                        code: error_code::HTTP_ERROR,
                        message: format!("{e}"),
                        data: None,
                    })
                }
            }
        }
        RpcMethod::WorkflowRun => {
            let params: WorkflowRunParams =
                serde_json::from_value(req.params.clone()).map_err(|e| RpcError {
                    code: error_code::INVALID_PARAMS,
                    message: format!("invalid workflow.run params: {e}"),
                    data: None,
                })?;
            match executor.run(&params.workflow, params.inputs).await {
                Ok(result) => {
                    let value = serde_json::to_value(&result).unwrap_or(Value::Null);
                    Ok(RpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(value),
                        error: None,
                    })
                }
                Err(e) => Err(e),
            }
        }
    }
}

fn parse_id(line: &str) -> RpcId {
    if let Ok(v) = serde_json::from_str::<Value>(line) {
        if let Some(id) = v.get("id") {
            if let Some(n) = id.as_i64() {
                return RpcId::Number(n);
            }
            if let Some(s) = id.as_str() {
                return RpcId::String(s.to_string());
            }
        }
    }
    warn!("failed to extract id from request");
    RpcId::Null
}
