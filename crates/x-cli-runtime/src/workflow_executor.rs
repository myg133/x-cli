//! Workflow 运行时执行器
//!
//! 按数组顺序执行 workflow 的每个 step，构造 call_params（用 InputRef 解析
//! 把 `$input.xxx` 和 `$steps.<name>.<path>` 替换成实际值），用 HttpCaller 调
//! 后端，把响应存到 step_outputs 供后续 step 引用。

use crate::http::HttpCaller;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tracing::warn;
use x_cli_core::ir::{ApiSpec, InputRef, StepInputs, Workflow};
use x_cli_core::protocol::{
    error_code, RpcError, WorkflowRunResult, WorkflowStepResult,
};

pub struct WorkflowExecutor {
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    base_url: Option<String>,
    http_caller: HttpCaller,
}

impl WorkflowExecutor {
    pub fn new(
        spec: Arc<ApiSpec>,
        workflows: BTreeMap<String, Arc<Workflow>>,
        base_url: Option<String>,
        http_caller: HttpCaller,
    ) -> Self {
        Self {
            spec,
            workflows,
            base_url,
            http_caller,
        }
    }

    pub fn http_caller(&self) -> &HttpCaller {
        &self.http_caller
    }

    pub fn base_url(&self) -> &Option<String> {
        &self.base_url
    }

    /// 执行一个 workflow
    pub async fn run(&self, name: &str, inputs: Value) -> Result<WorkflowRunResult, RpcError> {
        let workflow = self
            .workflows
            .get(name)
            .ok_or_else(|| RpcError {
                code: error_code::WORKFLOW_NOT_FOUND,
                message: format!("workflow not found: {name}"),
                data: None,
            })?
            .clone();

        // 校验外部 inputs
        let input_obj = inputs.as_object().cloned().unwrap_or_default();

        let mut step_outputs: HashMap<String, Value> = HashMap::new();
        let mut results: Vec<WorkflowStepResult> = Vec::new();

        for step in &workflow.steps {
            // 解析 step 的 inputs
            let call_params = match self.build_call_params(&step.inputs, &input_obj, &step_outputs) {
                Ok(p) => p,
                Err(e) => {
                    return Err(RpcError {
                        code: error_code::WORKFLOW_INPUT_INVALID,
                        message: format!("step `{}`: {e}", step.name),
                        data: None,
                    });
                }
            };

            // 找 endpoint
            let endpoint = self.spec.endpoints.get(&step.endpoint).ok_or_else(|| {
                RpcError {
                    code: error_code::ENDPOINT_NOT_FOUND,
                    message: format!(
                        "step `{}` references unknown endpoint `{}`",
                        step.name, step.endpoint
                    ),
                    data: None,
                }
            })?;

            // 调 HTTP
            let resp = match self
                .http_caller
                .call(
                    endpoint,
                    self.base_url.as_deref(),
                    &Value::Object(call_params.path_params),
                    &Value::Object(call_params.query),
                    &Value::Object(call_params.headers),
                    call_params.body.as_ref(),
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(step = %step.name, error = %e, "workflow step failed");
                    return Err(RpcError {
                        code: error_code::WORKFLOW_STEP_FAILED,
                        message: format!("step `{}` HTTP failed: {e}", step.name),
                        data: Some(serde_json::json!({
                            "step": step.name,
                            "endpoint": step.endpoint,
                        })),
                    });
                }
            };

            // 存输出（用 response 包装，方便路径引用 $steps.<name>.response.body.<path>）
            let body_for_next = resp.body.clone();
            step_outputs.insert(
                step.name.clone(),
                serde_json::json!({
                    "response": {
                        "status": resp.status,
                        "body": resp.body,
                    }
                }),
            );

            results.push(WorkflowStepResult {
                name: step.name.clone(),
                endpoint: step.endpoint.clone(),
                status: resp.status,
                body: resp.body,
            });

            // 4xx/5xx 也算 step 失败（按 HTTP 状态码）
            if resp.status >= 400 {
                return Err(RpcError {
                    code: error_code::WORKFLOW_STEP_FAILED,
                    message: format!(
                        "step `{}` returned {} (endpoint: {})",
                        step.name, resp.status, step.endpoint
                    ),
                    data: Some(serde_json::json!({
                        "step": step.name,
                        "status": resp.status,
                        "body": body_for_next,
                    })),
                });
            }
        }

        // outputs = 最后一步 body
        let outputs = results.last().map(|r| r.body.clone());

        Ok(WorkflowRunResult {
            status: "ok".to_string(),
            steps: results,
            outputs,
        })
    }

    /// 把 step.inputs 里所有 `$input.xxx` / `$steps.<name>.<path>` 替换成实际值
    fn build_call_params(
        &self,
        inputs: &StepInputs,
        input_obj: &Map<String, Value>,
        step_outputs: &HashMap<String, Value>,
    ) -> Result<CallParamsBuilt, String> {
        let path_params = resolve_map(&inputs.path_params, input_obj, step_outputs)?;
        let query = resolve_map(&inputs.query, input_obj, step_outputs)?;
        let headers = resolve_map(&inputs.headers, input_obj, step_outputs)?;
        // body 是 object map，组装成 object
        let body = if inputs.body.is_empty() {
            None
        } else {
            let mut obj = Map::new();
            for (k, v) in &inputs.body {
                let resolved = resolve_value(v, input_obj, step_outputs)?;
                obj.insert(k.clone(), resolved);
            }
            Some(Value::Object(obj))
        };
        Ok(CallParamsBuilt {
            path_params,
            query,
            headers,
            body,
        })
    }
}

struct CallParamsBuilt {
    path_params: Map<String, Value>,
    query: Map<String, Value>,
    headers: Map<String, Value>,
    body: Option<Value>,
}

fn resolve_map(
    src: &BTreeMap<String, String>,
    input_obj: &Map<String, Value>,
    step_outputs: &HashMap<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for (k, v) in src {
        let resolved = resolve_value(v, input_obj, step_outputs)?;
        out.insert(k.clone(), resolved);
    }
    Ok(out)
}

fn resolve_value(
    raw: &str,
    input_obj: &Map<String, Value>,
    step_outputs: &HashMap<String, Value>,
) -> Result<Value, String> {
    match InputRef::parse(raw) {
        InputRef::Input(name) => input_obj
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("missing workflow input `{name}`")),
        InputRef::StepOutput { step, path } => {
            let step_value = step_outputs
                .get(&step)
                .ok_or_else(|| format!("step output `{step}` not available"))?;
            lookup_path(step_value, &path)
                .ok_or_else(|| format!("path `{}` not found in step `{step}`", path.join(".")))
        }
        InputRef::Static(s) => Ok(Value::String(s)),
    }
}

fn lookup_path(root: &Value, path: &[String]) -> Option<Value> {
    let mut cur = root;
    for p in path {
        cur = cur.get(p.as_str())?;
    }
    Some(cur.clone())
}
