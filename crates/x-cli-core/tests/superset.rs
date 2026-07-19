//! Superset 真实数据驱动的回归测试
//!
//! 防止 IR / 解析在真实大型 OpenAPI 文档上静默出错。
//! Apache Superset 的 openapi.json：276 个接口、157 个 unique schema、305 个 $ref。

use x_cli_core::{parse_openapi_str_json, SchemaKind};

const SUPERSET: &str = include_str!("fixtures/superset.json");

#[test]
fn superset_parses_to_expected_count() {
    let spec = parse_openapi_str_json(SUPERSET).expect("parse superset");
    // 实际数：276 个 endpoint（不锁死，写宽松点：>200 且 < 400）
    let count = spec.endpoints.len();
    assert!(count > 200, "expected >200 endpoints, got {count}");
    assert!(count < 400, "expected <400 endpoints, got {count}");
}

#[test]
fn superset_domains_are_organized() {
    let spec = parse_openapi_str_json(SUPERSET).expect("parse");
    // 业务域 > 20
    assert!(spec.domains.len() > 20);
    // 域里有 endpoint
    for d in &spec.domains {
        assert!(!d.endpoint_ids.is_empty(), "domain {} has no endpoints", d.name);
    }
}

#[test]
fn superset_response_schema_resolves_to_real_name_not_object() {
    // 这是 B 阶段的真实缺口：未给 components.schemas.Pet 这类无 title 的 schema
    // 一个稳定的 schema name 是个修复。
    let spec = parse_openapi_str_json(SUPERSET).expect("parse");
    let ep = spec
        .endpoints
        .values()
        .find(|e| e.path == "/api/v1/advanced_data_type/convert" && e.method == x_cli_core::HttpMethod::Get)
        .expect("advanced_data_type convert endpoint");
    let resp_200 = ep
        .responses
        .iter()
        .find(|r| r.status == 200)
        .expect("200 response");
    let schema = resp_200.schema.as_ref().expect("schema");
    // 关键断言：schema name 不能是泛型 "object"
    assert_ne!(schema.name, "object", "schema name should not be generic 'object'");
    assert_ne!(schema.name, "any", "schema name should not be 'any'");
    // 必须含真实 schema 名（ref 解析应给到 AdvancedDataTypeSchema）
    assert!(
        schema.name.contains("AdvancedDataType"),
        "expected schema name with 'AdvancedDataType', got '{}'",
        schema.name
    );
    // 必须有解析后的 properties
    let resolved = schema.resolved.as_ref().expect("resolved");
    assert_eq!(resolved.kind, SchemaKind::Object);
    assert!(resolved.properties.contains_key("display_value"));
    assert!(resolved.properties.contains_key("values"));
}

#[test]
fn superset_database_post_request_body_has_all_fields() {
    // POST /api/v1/database/ 是个 ~20 字段的 schema，验证解析没漏字段
    let spec = parse_openapi_str_json(SUPERSET).expect("parse");
    let ep = spec
        .endpoints
        .values()
        .find(|e| e.path == "/api/v1/database/" && e.method == x_cli_core::HttpMethod::Post)
        .expect("create database endpoint");
    let rb = ep.request_body.as_ref().expect("request body");
    let resolved = rb.schema.resolved.as_ref().expect("resolved");
    // 关键字段都在
    for field in ["database_name", "sqlalchemy_uri", "allow_ctas", "allow_cvas", "extra"] {
        assert!(
            resolved.properties.contains_key(field),
            "expected field `{field}` in request body schema"
        );
    }
    // database_name 必填
    assert!(resolved.required.contains(&"database_name".to_string()));
}

#[test]
fn superset_no_recursive_overflow() {
    // 大型 OpenAPI 经常有循环 schema（如 database 引用 user，user 引用 database）。
    // 解析不能爆栈。
    let spec = parse_openapi_str_json(SUPERSET).expect("parse");
    // 简单断言：能完成解析即可
    let _ = spec.endpoints.len();
}

#[test]
fn superset_path_params_resolved() {
    // /api/v1/database/{pk} 之类的 endpoint 必须有 path param 解析
    let spec = parse_openapi_str_json(SUPERSET).expect("parse");
    let ep = spec
        .endpoints
        .values()
        .find(|e| e.path == "/api/v1/database/{pk}" && e.method == x_cli_core::HttpMethod::Get)
        .expect("get database by pk");
    let path_params: Vec<_> = ep
        .params
        .iter()
        .filter(|p| matches!(p.location, x_cli_core::ParamLocation::Path))
        .collect();
    assert_eq!(path_params.len(), 1);
    assert_eq!(path_params[0].name, "pk");
    assert!(path_params[0].required);
}

#[test]
fn oas3_0_parameters_content_style_gets_resolved() {
    // Superset 用 OAS 3.0 的 `parameters[].content` 风格，转换层必须把
    // `content.application/json.schema` 提到 `parameters[].schema`，
    // 否则 oas3 0.16 拿不到 schema，参数类型变成 any。
    let spec = parse_openapi_str_json(SUPERSET).expect("parse");
    let ep = spec
        .endpoints
        .values()
        .find(|e| e.path == "/api/v1/advanced_data_type/convert" && e.method == x_cli_core::HttpMethod::Get)
        .expect("advanced data type convert endpoint");
    let q = ep
        .params
        .iter()
        .find(|p| p.name == "q")
        .expect("q parameter");
    // schema name 应该是被解析的 schema 名，不是 any / object
    assert_ne!(q.schema.name, "any");
    assert!(
        q.schema.name.contains("advanced_data_type_convert"),
        "q schema name should contain 'advanced_data_type_convert', got '{}'",
        q.schema.name
    );
    // resolved 树应该有 properties（是个 Object schema）
    let resolved = q.schema.resolved.as_ref().expect("resolved");
    assert_eq!(resolved.kind, x_cli_core::SchemaKind::Object);
    // properties 包含 type 和 values（来自源 schema）
    assert!(resolved.properties.contains_key("type"));
    assert!(resolved.properties.contains_key("values"));
}
