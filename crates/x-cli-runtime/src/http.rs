//! HTTP 客户端 + 401 自动 retry
//!
//! HttpCaller 持有 [`Session`]（不是静态 AuthProfile）。每次调用：
//! 1. 从 Session 拿当前 headers 快照
//! 2. 发请求
//! 3. 如果 401 且 Session 配置了 `refresh.on_401` → 调 `Session::handle_401`
//!    成功 → 用新 headers 重试一次
//!    失败 / 静态 auth → 直接把 401 返回给调用方

use reqwest::{header::HeaderMap, header::HeaderName, header::HeaderValue, Method};
use serde_json::Value;
use std::time::Duration;
use x_cli_core::ir::Endpoint;

use crate::session::Session;

/// HTTP 调用器
#[derive(Clone)]
pub struct HttpCaller {
    client: reqwest::Client,
    session: Session,
}

impl HttpCaller {
    /// 用 Session 构造
    pub fn new(session: Session) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { client, session })
    }

    /// 拿到 Session 的当前 headers
    pub async fn session_headers(&self) -> std::collections::HashMap<String, String> {
        self.session.headers().await
    }

    /// 调用 endpoint
    ///
    /// `headers` 是 per-call 的额外 header（如 endpoint params 里的 header、agent 临时塞的），
    /// 优先级高于 Session 默认 header。
    pub async fn call(
        &self,
        endpoint: &Endpoint,
        base_url: Option<&str>,
        path_params: &Value,
        query: &Value,
        headers: &Value,
        body: Option<&Value>,
    ) -> anyhow::Result<HttpResponse> {
        let method = http_method(endpoint);
        let path = substitute_path(&endpoint.path, path_params);
        let url = match base_url {
            Some(b) => format!("{}{}", b.trim_end_matches('/'), path),
            None => path,
        };
        let query_pairs = build_query_pairs(query);
        let body_val = body.cloned();

        // 第一次请求
        let header_map = self.build_headers(headers).await;
        let mut resp = self
            .send_once(&method, &url, &query_pairs, header_map, body_val.as_ref())
            .await?;

        // 401 retry —— 一次,被 Session 的 loop guard 兜底
        if resp.status == 401 && self.session.handle_401().await? {
            let header_map = self.build_headers(headers).await;
            resp = self
                .send_once(&method, &url, &query_pairs, header_map, body_val.as_ref())
                .await?;
        }
        Ok(resp)
    }

    async fn build_headers(&self, per_call: &Value) -> HeaderMap {
        let mut header_map = HeaderMap::new();
        // 1. session headers（auth 兜底,优先级最低）
        for (k, v) in self.session.headers().await {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(&v),
            ) {
                header_map.insert(name, val);
            }
        }
        // 2. per-call headers（最高优先级,覆盖 session）
        if let Some(obj) = per_call.as_object() {
            for (k, v) in obj {
                if let (Ok(name), Ok(val)) = (
                    HeaderName::from_bytes(k.as_bytes()),
                    HeaderValue::from_str(&v.to_string()),
                ) {
                    header_map.insert(name, val);
                }
            }
        }
        header_map
    }

    async fn send_once(
        &self,
        method: &Method,
        url: &str,
        query: &[(String, String)],
        header_map: HeaderMap,
        body: Option<&Value>,
    ) -> anyhow::Result<HttpResponse> {
        let mut req = self.client.request(method.clone(), url);
        if !query.is_empty() {
            req = req.query(query);
        }
        req = req.headers(header_map);
        if let Some(b) = body {
            if !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS) {
                req = req.json(b);
            }
        }
        let resp = req
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("send request to {url}: {e}"))?;
        let status = resp.status().as_u16();
        let resp_headers: Value = {
            let mut m = serde_json::Map::new();
            for (k, v) in resp.headers() {
                if let Ok(s) = v.to_str() {
                    m.insert(k.as_str().to_string(), Value::String(s.to_string()));
                }
            }
            Value::Object(m)
        };
        let resp_body: Value = resp.json().await.unwrap_or(Value::Null);
        Ok(HttpResponse {
            status,
            headers: resp_headers,
            body: resp_body,
        })
    }
}

fn http_method(endpoint: &Endpoint) -> Method {
    match endpoint.method {
        x_cli_core::ir::HttpMethod::Get => Method::GET,
        x_cli_core::ir::HttpMethod::Post => Method::POST,
        x_cli_core::ir::HttpMethod::Put => Method::PUT,
        x_cli_core::ir::HttpMethod::Patch => Method::PATCH,
        x_cli_core::ir::HttpMethod::Delete => Method::DELETE,
        x_cli_core::ir::HttpMethod::Head => Method::HEAD,
        x_cli_core::ir::HttpMethod::Options => Method::OPTIONS,
    }
}

fn build_query_pairs(query: &Value) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    if let Some(obj) = query.as_object() {
        for (k, v) in obj {
            let s = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            pairs.push((k.clone(), s));
        }
    }
    pairs
}

/// HTTP 响应（从 HttpResponseExtractor 解析后的结构化结果）。
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP 状态码
    pub status: u16,
    /// 响应头（JSON 对象）
    pub headers: Value,
    /// 响应体（JSON）
    pub body: Value,
}

fn substitute_path(path: &str, params: &Value) -> String {
    let mut out = path.to_string();
    if let Some(obj) = params.as_object() {
        for (k, v) in obj {
            let placeholder = format!("{{{k}}}");
            let s = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out = out.replace(&placeholder, &s);
        }
    }
    out
}
