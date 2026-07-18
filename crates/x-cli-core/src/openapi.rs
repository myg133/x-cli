//! OpenAPI 解析
//!
//! 把 `oas3::OpenApiV3Spec` 转换为我们的 IR `ApiSpec`。
//! A 阶段：扁平化所有 path + method，归类到 tag 域，参数/请求体/响应只取第一个 schema。
//! B 阶段可加：参数冲突消解、$ref 解析、$ref 循环检测、allOf/oneOf 处理。

use crate::error::{Error, Result};
use crate::ir::{
    ApiSpec, Domain, Endpoint, HttpMethod, Param, ParamLocation, RequestBody, Response, SchemaRef,
};
use oas3::spec::{
    ObjectOrReference, ObjectSchema, Operation, ParameterIn, PathItem, SchemaTypeSet, Server,
};
use oas3::{OpenApiV3Spec, Spec as OasSpec};
use std::collections::BTreeMap;
use std::path::Path;

/// 从文件读取并解析 OpenAPI 3 文档
pub fn parse_openapi<P: AsRef<Path>>(path: P) -> Result<ApiSpec> {
    let content = std::fs::read_to_string(path.as_ref())?;
    parse_openapi_str(&content)
}

/// 从 YAML 字符串解析 OpenAPI 3 文档
pub fn parse_openapi_str(yaml: &str) -> Result<ApiSpec> {
    let spec: OpenApiV3Spec = oas3::from_yaml(yaml).map_err(|e| Error::OpenApiParse(e.to_string()))?;
    Ok(convert(spec))
}

fn convert(spec: OasSpec) -> ApiSpec {
    let title = spec.info.title.clone();
    let version = spec.info.version.clone();
    let description = spec
        .info
        .description
        .clone()
        .filter(|d| !d.is_empty());

    let base_url = first_server_url(&spec.servers);

    let mut endpoints: BTreeMap<String, Endpoint> = BTreeMap::new();
    let mut by_tag: BTreeMap<String, Vec<String>> = BTreeMap::new();

    if let Some(paths) = &spec.paths {
        for (path, path_item) in paths {
            let methods: [(HttpMethod, &Option<Operation>); 7] = [
                (HttpMethod::Get, &path_item.get),
                (HttpMethod::Post, &path_item.post),
                (HttpMethod::Put, &path_item.put),
                (HttpMethod::Patch, &path_item.patch),
                (HttpMethod::Delete, &path_item.delete),
                (HttpMethod::Head, &path_item.head),
                (HttpMethod::Options, &path_item.options),
            ];

            for (method, op_opt) in methods {
                let Some(op) = op_opt else { continue };
                let domain = pick_domain(op, path);
                let id = make_endpoint_id(&domain, method, path);

                let endpoint = Endpoint {
                    id: id.clone(),
                    domain: domain.clone(),
                    method,
                    path: path.clone(),
                    operation_id: op.operation_id.clone(),
                    summary: op.summary.clone(),
                    description: op.description.clone(),
                    tags: op.tags.clone(),
                    params: convert_params(&op.parameters),
                    request_body: convert_request_body(op.request_body.as_ref()),
                    responses: convert_responses(op.responses.as_ref()),
                    deprecated: op.deprecated.unwrap_or(false),
                };

                endpoints.insert(id.clone(), endpoint);
                by_tag.entry(domain).or_default().push(id);
            }
        }
    }

    let domains = by_tag
        .into_iter()
        .map(|(name, endpoint_ids)| Domain {
            name,
            description: None,
            endpoint_ids,
        })
        .collect();

    ApiSpec {
        title,
        version,
        description,
        base_url,
        domains,
        endpoints,
    }
}

fn first_server_url(servers: &[Server]) -> Option<String> {
    let url = servers.first()?.url.clone();
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

/// 选 domain：优先用第一个 tag；都没有用 path 第一段；最终兜底 "default"
fn pick_domain(op: &Operation, path: &str) -> String {
    if let Some(t) = op.tags.first() {
        return t.clone();
    }
    let seg = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('{'));
    seg.unwrap_or_else(|| "default".to_string())
}

fn make_endpoint_id(domain: &str, method: HttpMethod, path: &str) -> String {
    let sanitized_path = path
        .trim_start_matches('/')
        .replace('/', "_")
        .replace('{', "")
        .replace('}', "");
    let method_str = match method {
        HttpMethod::Get => "get",
        HttpMethod::Post => "post",
        HttpMethod::Put => "put",
        HttpMethod::Patch => "patch",
        HttpMethod::Delete => "delete",
        HttpMethod::Head => "head",
        HttpMethod::Options => "options",
    };
    format!("{domain}__{method_str}__{sanitized_path}")
}

fn convert_params(params: &[ObjectOrReference<oas3::spec::Parameter>]) -> Vec<Param> {
    params
        .iter()
        .filter_map(|oor| match oor {
            ObjectOrReference::Object(p) => Some(convert_one_param(p)),
            ObjectOrReference::Ref { .. } => None, // A 阶段：跳过 $ref
        })
        .collect()
}

fn convert_one_param(p: &oas3::spec::Parameter) -> Param {
    let location = match p.location {
        ParameterIn::Query => ParamLocation::Query,
        ParameterIn::Path => ParamLocation::Path,
        ParameterIn::Header => ParamLocation::Header,
        ParameterIn::Cookie => ParamLocation::Cookie,
    };
    Param {
        name: p.name.clone(),
        location,
        required: p.required.unwrap_or(false),
        description: p.description.clone(),
        schema: schema_from_oor(&p.schema),
    }
}

fn convert_request_body(
    rb: Option<&ObjectOrReference<oas3::spec::RequestBody>>,
) -> Option<RequestBody> {
    let oor = rb?;
    let rb = match oor {
        ObjectOrReference::Object(r) => r,
        ObjectOrReference::Ref { .. } => return None, // A 阶段：跳过 $ref
    };
    let (content_type, media) = rb.content.iter().next()?;
    Some(RequestBody {
        required: rb.required.unwrap_or(false),
        content_type: content_type.clone(),
        schema: schema_from_oor(&media.schema),
    })
}

fn convert_responses(
    responses: Option<&BTreeMap<String, ObjectOrReference<oas3::spec::Response>>>,
) -> Vec<Response> {
    let Some(responses) = responses else { return Vec::new() };
    let mut out: Vec<Response> = responses
        .iter()
        .filter_map(|(status_str, oor)| {
            let resp = match oor {
                ObjectOrReference::Object(r) => r,
                ObjectOrReference::Ref { .. } => return None,
            };
            let status: u16 = status_str.parse().unwrap_or(0);
            let (content_type, schema) = resp
                .content
                .iter()
                .next()
                .map(|(ct, m)| (Some(ct.clone()), Some(schema_from_oor(&m.schema))))
                .unwrap_or((None, None));
            Some(Response {
                status,
                description: resp.description.clone(),
                content_type,
                schema,
            })
        })
        .collect();
    out.sort_by_key(|r| r.status);
    out
}

fn schema_from_oor(oor: &Option<ObjectOrReference<ObjectSchema>>) -> SchemaRef {
    let Some(oor) = oor else {
        return SchemaRef::any();
    };
    match oor {
        ObjectOrReference::Object(s) => schema_from_object(s),
        ObjectOrReference::Ref { ref_path } => {
            let display = ref_path.to_string();
            SchemaRef {
                name: ref_display_name(&display),
                description: Some(format!("$ref: {display}")),
                json_schema: serde_json::json!({ "$ref": display }),
            }
        }
    }
}

fn schema_from_object(s: &ObjectSchema) -> SchemaRef {
    let name = s
        .title
        .clone()
        .or_else(|| Some(type_set_label(&s.schema_type)))
        .unwrap_or_else(|| "any".to_string());
    let description = s.description.clone();
    let json_schema = serde_json::to_value(s).unwrap_or(serde_json::json!({}));
    SchemaRef {
        name,
        description,
        json_schema,
    }
}

fn type_set_label(ts: &Option<SchemaTypeSet>) -> String {
    let Some(ts) = ts else { return "any".to_string() };
    match ts {
        SchemaTypeSet::Single(t) => format!("{:?}", t).to_lowercase(),
        SchemaTypeSet::Multiple(ts) => ts
            .iter()
            .map(|t| format!("{:?}", t).to_lowercase())
            .collect::<Vec<_>>()
            .join("|"),
    }
}

fn ref_display_name(reference: &str) -> String {
    reference
        .rsplit('/')
        .next()
        .unwrap_or("ref")
        .to_string()
}

#[allow(dead_code)]
fn _path_item_marker(_p: &PathItem) {} // 防止 PathItem import 被 unused
