//! Build an `OpenAPI` 3.1 document from parsed controller metadata.

use std::collections::BTreeMap;

use anvil_bellows::{ControllerFile, Controller, HttpMethod, ParamKind, ReturnKind};
use serde_json::{json, Value};

use crate::config::OpenApiConfig;

/// A non-fatal diagnostic emitted during `OpenAPI` generation.
#[derive(Debug, Clone)]
pub struct BuildDiagnostic {
    /// Human-readable message.
    pub message: String,
}

/// Build an `OpenAPI` 3.1 document from parsed controller files.
///
/// Unknown `@Security` schemes (not present in `config.security_schemes`) emit
/// a [`BuildDiagnostic`]; the scheme is still included in the output.
pub fn build_openapi(
    files: &[ControllerFile],
    config: &OpenApiConfig,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> Value {
    let mut paths: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();

    for file in files {
        for ctrl in &file.controllers {
            for route in &ctrl.routes {
                let (openapi_path, path_param_names) =
                    express_to_openapi_path(&route.path);
                let method_key = http_method_key(&route.method);
                let op = build_operation(ctrl, route, &path_param_names, config, diagnostics);
                paths.entry(openapi_path).or_default().insert(method_key, op);
            }
        }
    }

    assemble_doc(paths, config)
}

fn build_operation(
    ctrl: &Controller,
    route: &anvil_bellows::Route,
    path_param_names: &[String],
    config: &OpenApiConfig,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> Value {
    let op_id = operation_id(&ctrl.class_name, &route.method, &route.handler_name);

    let tags: Vec<Value> = if ctrl.tags.is_empty() {
        vec![json!(default_tag(&ctrl.class_name))]
    } else {
        ctrl.tags.iter().map(|t| json!(t)).collect()
    };

    let parameters = build_parameters(path_param_names, &route.params);
    let request_body = build_request_body(&route.params);
    let responses = build_responses(route);
    let security = build_security(&ctrl.security, config, diagnostics);

    let mut op = serde_json::Map::new();
    op.insert("operationId".into(), json!(op_id));
    op.insert("tags".into(), json!(tags));
    if route.deprecated {
        op.insert("deprecated".into(), json!(true));
    }
    if !parameters.is_empty() {
        op.insert("parameters".into(), json!(parameters));
    }
    if let Some(rb) = request_body {
        op.insert("requestBody".into(), rb);
    }
    op.insert("responses".into(), json!(responses));
    if let Some(sec) = security {
        op.insert("security".into(), json!(sec));
    }
    Value::Object(op)
}

fn build_parameters(
    path_param_names: &[String],
    params: &[anvil_bellows::TypedParam],
) -> Vec<Value> {
    let mut parameters: Vec<Value> = path_param_names
        .iter()
        .map(|pname| json!({ "name": pname, "in": "path", "required": true, "schema": {} }))
        .collect();

    for param in params {
        if matches!(param.kind, ParamKind::Query(_)) {
            parameters.push(json!({
                "name": param.name,
                "in": "query",
                "required": false,
                "schema": {}
            }));
        }
    }
    parameters
}

fn build_request_body(params: &[anvil_bellows::TypedParam]) -> Option<Value> {
    params.iter().find_map(|p| {
        if matches!(p.kind, ParamKind::Body(_)) {
            Some(json!({
                "required": true,
                "content": { "application/json": { "schema": {} } }
            }))
        } else {
            None
        }
    })
}

fn build_responses(route: &anvil_bellows::Route) -> BTreeMap<String, Value> {
    let mut responses: BTreeMap<String, Value> = BTreeMap::new();
    let success = match &route.return_kind {
        ReturnKind::Responds { .. } => json!({
            "description": "Success",
            "content": { "application/json": { "schema": {} } }
        }),
        ReturnKind::Void { .. } => json!({ "description": "Success" }),
    };
    responses.insert("200".to_owned(), success);
    for er in &route.extra_responses {
        responses.insert(
            er.status.to_string(),
            json!({ "description": http_status_description(er.status) }),
        );
    }
    responses
}

fn build_security(
    security: &[String],
    config: &OpenApiConfig,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> Option<Vec<Value>> {
    if security.is_empty() {
        return None;
    }
    let reqs: Vec<Value> = security
        .iter()
        .map(|scheme| {
            if !config.security_schemes.contains_key(scheme.as_str()) {
                diagnostics.push(BuildDiagnostic {
                    message: format!(
                        "@Security(\"{scheme}\") is not declared in \
                         securitySchemes; add it to the config file."
                    ),
                });
            }
            json!({ scheme: [] })
        })
        .collect();
    Some(reqs)
}

fn assemble_doc(
    paths: BTreeMap<String, BTreeMap<String, Value>>,
    config: &OpenApiConfig,
) -> Value {
    let mut doc = serde_json::Map::new();
    doc.insert("openapi".into(), json!("3.1.0"));
    doc.insert("info".into(), json!({ "title": config.info.title, "version": config.info.version }));

    if !config.servers.is_empty() {
        let servers: Vec<Value> = config.servers.iter().map(|url| json!({ "url": url })).collect();
        doc.insert("servers".into(), json!(servers));
    }

    let paths_val: serde_json::Map<String, Value> = paths
        .into_iter()
        .map(|(path, methods)| (path, Value::Object(methods.into_iter().collect())))
        .collect();
    doc.insert("paths".into(), Value::Object(paths_val));

    let security_schemes_val: BTreeMap<String, Value> = config
        .security_schemes
        .iter()
        .map(|(name, scheme)| {
            let mut obj = serde_json::Map::new();
            obj.insert("type".into(), json!(scheme.r#type));
            if let Some(s) = &scheme.scheme { obj.insert("scheme".into(), json!(s)); }
            if let Some(n) = &scheme.name { obj.insert("name".into(), json!(n)); }
            if let Some(r#in) = &scheme.r#in { obj.insert("in".into(), json!(r#in)); }
            (name.clone(), Value::Object(obj))
        })
        .collect();

    if !security_schemes_val.is_empty() {
        let schemes_map: serde_json::Map<String, Value> = security_schemes_val.into_iter().collect();
        doc.insert("components".into(), json!({ "securitySchemes": Value::Object(schemes_map) }));
    }

    Value::Object(doc)
}

/// Convert an Express-style path (`/users/:id`) to `OpenAPI` format (`/users/{id}`)
/// and extract path parameter names.
fn express_to_openapi_path(path: &str) -> (String, Vec<String>) {
    let mut params = Vec::new();
    let segments: Vec<String> = path
        .split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                params.push(name.to_owned());
                format!("{{{name}}}")
            } else {
                seg.to_owned()
            }
        })
        .collect();
    (segments.join("/"), params)
}

fn http_method_key(method: &HttpMethod) -> String {
    method.http_verb().to_lowercase()
}

/// Derive the operation ID: `lowerFirst(class) + Method + UpperFirst(handler)`.
fn operation_id(class_name: &str, method: &HttpMethod, handler: &str) -> String {
    format!(
        "{}{}{}",
        lower_first(class_name),
        method.decorator_name(),
        upper_first(handler)
    )
}

/// Default tag: class name without the `Controller` suffix, lowercased.
fn default_tag(class_name: &str) -> String {
    class_name
        .strip_suffix("Controller")
        .unwrap_or(class_name)
        .to_lowercase()
}

fn http_status_description(status: u16) -> &'static str {
    match status {
        200 => "Success",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        _ => "Response",
    }
}

fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().to_string() + chars.as_str(),
    }
}

fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn express_path_to_openapi() {
        let (p, params) = express_to_openapi_path("/users/:id");
        assert_eq!(p, "/users/{id}");
        assert_eq!(params, ["id"]);

        let (p, params) = express_to_openapi_path("/users/:userId/posts/:postId");
        assert_eq!(p, "/users/{userId}/posts/{postId}");
        assert_eq!(params, ["userId", "postId"]);

        let (p, params) = express_to_openapi_path("/health");
        assert_eq!(p, "/health");
        assert!(params.is_empty());
    }

    #[test]
    fn default_tag_strips_controller_suffix() {
        assert_eq!(default_tag("UserController"), "user");
        assert_eq!(default_tag("OrderController"), "order");
        assert_eq!(default_tag("Health"), "health");
    }

    #[test]
    fn operation_id_format() {
        assert_eq!(
            operation_id("UserController", &HttpMethod::Get, "byId"),
            "userControllerGetById"
        );
        assert_eq!(
            operation_id("OrderController", &HttpMethod::Post, "create"),
            "orderControllerPostCreate"
        );
    }
}
