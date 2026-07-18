//! HTTP 客户端
//!
//! A 阶段：reqwest + 一个 auth profile，后面 B 阶段加多 profile 切换、连接池配置、
//! 请求/响应拦截器、mock 模式。

use reqwest::{header::HeaderMap, header::HeaderName, header::HeaderValue, Method};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use x_cli_core::ir::Endpoint;

/// 鉴权 profile
#[derive(Debug, Clone, Default)]
pub struct AuthProfile {
    /// 注入到所有请求头（如 Authorization: Bearer xxx）
    pub headers: HashMap<String, String>,
}

impl AuthProfile {
    /// 从环境变量构造
    pub fn from_env(env_key: &str, header_name: &str, prefix: &str) -> Self {
        let mut headers = HashMap::new();
        if let Ok(v) = std::env::var(env_key) {
            headers.insert(header_name.to_string(), format!("{prefix}{v}"));
        }
        Self { headers }
    }
}

/// HTTP 调用器
#[derive(Clone)]
pub struct HttpCaller {
    client: reqwest::Client,
    auth: AuthProfile,
}

impl HttpCaller {
    pub fn new(auth: AuthProfile) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { client, auth })
    }

    /// 调用 endpoint
    ///
    /// `base_url` 一般是 `ApiSpec.base_url`。
    /// `path_params` 替换 path 里的 `{xxx}`。
    /// `query` 拼到 query string。
    /// `headers` 合并到请求头，auth profile 优先级最低。
    /// `body` 是 JSON，POST/PUT/PATCH 时使用。
    pub async fn call(
        &self,
        endpoint: &Endpoint,
        base_url: Option<&str>,
        path_params: &Value,
        query: &Value,
        headers: &Value,
        body: Option<&Value>,
    ) -> anyhow::Result<HttpResponse> {
        let method = match endpoint.method {
            x_cli_core::ir::HttpMethod::Get => Method::GET,
            x_cli_core::ir::HttpMethod::Post => Method::POST,
            x_cli_core::ir::HttpMethod::Put => Method::PUT,
            x_cli_core::ir::HttpMethod::Patch => Method::PATCH,
            x_cli_core::ir::HttpMethod::Delete => Method::DELETE,
            x_cli_core::ir::HttpMethod::Head => Method::HEAD,
            x_cli_core::ir::HttpMethod::Options => Method::OPTIONS,
        };

        // 1. 拼 URL
        let path = substitute_path(&endpoint.path, path_params);
        let url = match base_url {
            Some(b) => format!("{}{}", b.trim_end_matches('/'), path),
            None => path,
        };

        // 2. 构造请求
        let mut req = self.client.request(method, &url);

        // 3. query
        if let Some(obj) = query.as_object() {
            let mut pairs: Vec<(String, String)> = Vec::new();
            for (k, v) in obj {
                let s = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                pairs.push((k.clone(), s));
            }
            req = req.query(&pairs);
        }

        // 4. headers: auth < endpoint < per-call
        let mut header_map = HeaderMap::new();
        for (k, v) in &self.auth.headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                header_map.insert(name, val);
            }
        }
        // 端点参数里 location=Header 的也加进去
        for p in &endpoint.params {
            if matches!(p.location, x_cli_core::ir::ParamLocation::Header) {
                if let Some(obj) = headers.as_object() {
                    if let Some(v) = obj.get(&p.name) {
                        let s = match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        if let (Ok(name), Ok(val)) = (
                            HeaderName::from_bytes(p.name.as_bytes()),
                            HeaderValue::from_str(&s),
                        ) {
                            header_map.insert(name, val);
                        }
                    }
                }
            }
        }
        // per-call 额外 headers
        if let Some(obj) = headers.as_object() {
            for (k, v) in obj {
                if let (Ok(name), Ok(val)) = (
                    HeaderName::from_bytes(k.as_bytes()),
                    HeaderValue::from_str(&v.to_string()),
                ) {
                    header_map.insert(name, val);
                }
            }
        }
        req = req.headers(header_map);

        // 5. body
        if let Some(b) = body {
            if !matches!(
                endpoint.method,
                x_cli_core::ir::HttpMethod::Get | x_cli_core::ir::HttpMethod::Head
            ) {
                req = req.json(b);
            }
        }

        // 6. 发请求
        let resp = req.send().await?;
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

/// HTTP 响应
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Value,
    pub body: Value,
}

/// 把 `{xxx}` 替换成 path_params 里的值
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
