//! Build an `OpenAPI` 3.1 document from parsed controller metadata.

use std::collections::{BTreeMap, BTreeSet};

use anvil_bellows::{Controller, ControllerFile, HttpMethod, ParamKind, ReturnKind};
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
///
/// Every `Body<S>`, `Query<S>`, and `Responds<S>` schema identifier is
/// collected and emitted as a named stub under `components/schemas`, with
/// `$ref` pointers wired into the appropriate request/response locations.
pub fn build_openapi(
    files: &[ControllerFile],
    config: &OpenApiConfig,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> Value {
    let mut paths: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut schemas: BTreeSet<String> = BTreeSet::new();

    for file in files {
        for ctrl in &file.controllers {
            for route in &ctrl.routes {
                let (openapi_path, path_param_names) = express_to_openapi_path(&route.path);
                let method_key = http_method_key(&route.method);
                let op = build_operation(
                    ctrl,
                    route,
                    &path_param_names,
                    config,
                    diagnostics,
                    &mut schemas,
                );
                paths
                    .entry(openapi_path)
                    .or_default()
                    .insert(method_key, op);
            }
        }
    }

    assemble_doc(paths, config, schemas)
}

fn build_operation(
    ctrl: &Controller,
    route: &anvil_bellows::Route,
    path_param_names: &[String],
    config: &OpenApiConfig,
    diagnostics: &mut Vec<BuildDiagnostic>,
    schemas: &mut BTreeSet<String>,
) -> Value {
    let op_id = operation_id(&ctrl.class_name, &route.method, &route.handler_name);

    let tags: Vec<Value> = if ctrl.tags.is_empty() {
        vec![json!(default_tag(&ctrl.class_name))]
    } else {
        ctrl.tags.iter().map(|t| json!(t)).collect()
    };

    let parameters = build_parameters(path_param_names, &route.params, schemas);
    let request_body = build_request_body(&route.params, schemas, diagnostics);
    let responses = build_responses(route, schemas, diagnostics);
    let mut scheme_names = ctrl.security.clone();
    scheme_names.extend(route.authn.iter().filter_map(|a| a.scheme.clone()));
    let security = build_security(&scheme_names, config, diagnostics);

    // Use BTreeMap so operation keys are alphabetically sorted regardless of
    // whether the serde_json `preserve_order` feature is active in this build.
    let mut op: BTreeMap<String, Value> = BTreeMap::new();
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
    json!(op)
}

fn schema_ref(ident: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{ident}") })
}

fn build_parameters(
    path_param_names: &[String],
    params: &[anvil_bellows::TypedParam],
    schemas: &mut BTreeSet<String>,
) -> Vec<Value> {
    // Path params are always plain strings — no Zod schema covers them individually.
    let mut parameters: Vec<Value> = path_param_names
        .iter()
        .map(|pname| {
            // BTreeMap keeps "in", "name", "required", "schema" in alphabetical order.
            let mut p: BTreeMap<String, Value> = BTreeMap::new();
            p.insert("in".into(), json!("path"));
            p.insert("name".into(), json!(pname));
            p.insert("required".into(), json!(true));
            p.insert("schema".into(), json!({ "type": "string" }));
            json!(p)
        })
        .collect();

    for param in params {
        if let ParamKind::Query(s) = &param.kind {
            schemas.insert(s.ident.clone());
            let mut p: BTreeMap<String, Value> = BTreeMap::new();
            p.insert("in".into(), json!("query"));
            p.insert("name".into(), json!(param.name));
            p.insert("required".into(), json!(false));
            p.insert("schema".into(), schema_ref(&s.ident));
            parameters.push(json!(p));
        }
    }
    for param in params {
        if let ParamKind::Headers(s) = &param.kind {
            schemas.insert(s.ident.clone());
            let mut p: BTreeMap<String, Value> = BTreeMap::new();
            p.insert("in".into(), json!("header"));
            p.insert("name".into(), json!(param.name));
            p.insert("required".into(), json!(false));
            p.insert("schema".into(), schema_ref(&s.ident));
            parameters.push(json!(p));
        }
    }
    parameters
}

fn build_request_body(
    params: &[anvil_bellows::TypedParam],
    schemas: &mut BTreeSet<String>,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> Option<Value> {
    params.iter().find_map(|p| {
        let (s, content_type) = match &p.kind {
            ParamKind::Body(s) => (s, "application/json".to_owned()),
            ParamKind::FormBody(s) => (s, "application/x-www-form-urlencoded".to_owned()),
            ParamKind::Consumes { schema, codec } => {
                let content_type = codec.content_type.clone().unwrap_or_else(|| {
                    diagnostics.push(BuildDiagnostic {
                        message: format!(
                            "Body<typeof {}, typeof {}>'s contentType isn't statically \
                             resolvable (the codec must be a top-level `const` object \
                             literal in the same file); defaulting to \
                             application/octet-stream in the OpenAPI spec.",
                            schema.ident, codec.ident
                        ),
                    });
                    "application/octet-stream".to_owned()
                });
                (schema, content_type)
            }
            _ => return None,
        };
        schemas.insert(s.ident.clone());
        let mut rb: BTreeMap<String, Value> = BTreeMap::new();
        rb.insert(
            "content".into(),
            json!({ content_type: { "schema": schema_ref(&s.ident) } }),
        );
        rb.insert("required".into(), json!(true));
        Some(json!(rb))
    })
}

fn build_responses(
    route: &anvil_bellows::Route,
    schemas: &mut BTreeSet<String>,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> BTreeMap<String, Value> {
    let mut responses: BTreeMap<String, Value> = BTreeMap::new();
    let success = match &route.return_kind {
        ReturnKind::Responds { schema, codec, .. } => {
            schemas.insert(schema.ident.clone());
            let content_type = codec.as_ref().map_or("application/json", |c| {
                c.content_type.as_deref().unwrap_or_else(|| {
                    diagnostics.push(BuildDiagnostic {
                        message: format!(
                            "Produces<{}, typeof {}>'s contentType isn't statically \
                             resolvable (the codec must be a top-level `const` object \
                             literal in the same file); defaulting to application/json \
                             in the OpenAPI spec.",
                            schema.ident, c.ident
                        ),
                    });
                    "application/json"
                })
            });
            // BTreeMap keeps "content" before "description" (alphabetical).
            let mut r: BTreeMap<String, Value> = BTreeMap::new();
            r.insert(
                "content".into(),
                json!({ content_type: { "schema": schema_ref(&schema.ident) } }),
            );
            r.insert("description".into(), json!("Success"));
            json!(r)
        }
        ReturnKind::Void { .. } if route.is_sse => json!({
            "content": { "text/event-stream": { "schema": { "type": "string" } } },
            "description": "Server-sent event stream",
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
    schemas: BTreeSet<String>,
) -> Value {
    // Use BTreeMap for the top-level document so keys are alphabetically
    // sorted regardless of insertion order — this keeps golden files stable.
    let mut doc: BTreeMap<String, Value> = BTreeMap::new();
    doc.insert("openapi".into(), json!("3.1.0"));
    doc.insert(
        "info".into(),
        json!({ "title": config.info.title, "version": config.info.version }),
    );

    if !config.servers.is_empty() {
        let servers: Vec<Value> = config
            .servers
            .iter()
            .map(|url| json!({ "url": url }))
            .collect();
        doc.insert("servers".into(), json!(servers));
    }

    let paths_val: serde_json::Map<String, Value> = paths
        .into_iter()
        .map(|(path, methods)| (path, Value::Object(methods.into_iter().collect())))
        .collect();
    doc.insert("paths".into(), Value::Object(paths_val));

    // Accumulate both schemas and security schemes under `components`.
    let schemas_map: BTreeMap<String, Value> = schemas
        .into_iter()
        .map(|ident| (ident, json!({})))
        .collect();

    let security_schemes_val: BTreeMap<String, Value> = config
        .security_schemes
        .iter()
        .map(|(name, scheme)| {
            let mut obj: BTreeMap<String, Value> = BTreeMap::new();
            obj.insert("type".into(), json!(scheme.r#type));
            if let Some(s) = &scheme.scheme {
                obj.insert("scheme".into(), json!(s));
            }
            if let Some(n) = &scheme.name {
                obj.insert("name".into(), json!(n));
            }
            if let Some(r#in) = &scheme.r#in {
                obj.insert("in".into(), json!(r#in));
            }
            (name.clone(), json!(obj))
        })
        .collect();

    if !schemas_map.is_empty() || !security_schemes_val.is_empty() {
        let mut components: BTreeMap<String, Value> = BTreeMap::new();
        if !schemas_map.is_empty() {
            components.insert("schemas".into(), json!(schemas_map));
        }
        if !security_schemes_val.is_empty() {
            components.insert("securitySchemes".into(), json!(security_schemes_val));
        }
        doc.insert("components".into(), json!(components));
    }

    json!(doc)
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

    #[test]
    fn authn_scheme_merges_into_operation_security() {
        use crate::config::{InfoConfig, SecuritySchemeConfig};
        use anvil_bellows::{AuthRef, ReturnKind as RK, Route};
        use std::path::PathBuf;

        let ctrl = Controller {
            class_name: "AdminController".into(),
            ctor_params: vec![],
            tags: vec![],
            security: vec![],
            routes: vec![Route {
                method: HttpMethod::Get,
                path: "/admin/stats".into(),
                handler_name: "stats".into(),
                is_sse: false,
                params: vec![],
                return_kind: RK::Void { is_async: false },
                deprecated: false,
                extra_responses: vec![],
                middleware: vec![],
                authn: vec![AuthRef {
                    name: "SessionAuthn".into(),
                    origin: None,
                    scheme: Some("bearerAuth".into()),
                    user_identity: None,
                }],
                authz: vec![],
            }],
        };
        let files = vec![ControllerFile {
            source_path: PathBuf::from("/project/src/admin-controller.ts"),
            controllers: vec![ctrl],
        }];

        let mut config = OpenApiConfig {
            info: InfoConfig {
                title: "API".into(),
                version: "1.0.0".into(),
            },
            servers: vec![],
            security_schemes: BTreeMap::new(),
        };
        config.security_schemes.insert(
            "bearerAuth".into(),
            SecuritySchemeConfig {
                r#type: "http".into(),
                scheme: Some("bearer".into()),
                name: None,
                r#in: None,
            },
        );

        let mut diagnostics = Vec::new();
        let doc = build_openapi(&files, &config, &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let security = &doc["paths"]["/admin/stats"]["get"]["security"];
        assert_eq!(json!(security), json!([{ "bearerAuth": [] }]));
    }

    #[test]
    fn form_body_and_headers_emit_form_content_type_and_header_param() {
        use anvil_bellows::{ParamKind, ReturnKind as RK, Route, SchemaRef, TypedParam};
        use std::path::PathBuf;

        let ctrl = Controller {
            class_name: "WebhooksController".into(),
            ctor_params: vec![],
            tags: vec![],
            security: vec![],
            routes: vec![Route {
                method: HttpMethod::Post,
                path: "/webhooks/gather".into(),
                handler_name: "gather".into(),
                is_sse: false,
                params: vec![
                    TypedParam {
                        name: "body".into(),
                        kind: ParamKind::FormBody(SchemaRef {
                            ident: "GatherBody".into(),
                        }),
                    },
                    TypedParam {
                        name: "headers".into(),
                        kind: ParamKind::Headers(SchemaRef {
                            ident: "SignatureHeaders".into(),
                        }),
                    },
                ],
                return_kind: RK::Void { is_async: false },
                deprecated: false,
                extra_responses: vec![],
                middleware: vec![],
                authn: vec![],
                authz: vec![],
            }],
        };
        let files = vec![ControllerFile {
            source_path: PathBuf::from("/project/src/webhooks-controller.ts"),
            controllers: vec![ctrl],
        }];

        let config = OpenApiConfig {
            info: crate::config::InfoConfig {
                title: "API".into(),
                version: "1.0.0".into(),
            },
            servers: vec![],
            security_schemes: BTreeMap::new(),
        };

        let mut diagnostics = Vec::new();
        let doc = build_openapi(&files, &config, &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let op = &doc["paths"]["/webhooks/gather"]["post"];
        assert_eq!(
            op["requestBody"]["content"]["application/x-www-form-urlencoded"]["schema"]["$ref"],
            json!("#/components/schemas/GatherBody")
        );
        assert!(op["requestBody"]["content"]["application/json"].is_null());

        let params = op["parameters"].as_array().expect("parameters array");
        assert!(params.iter().any(|p| p["in"] == "header"
            && p["name"] == "headers"
            && p["schema"]["$ref"] == "#/components/schemas/SignatureHeaders"));
    }

    #[test]
    fn consumes_emits_codecs_resolved_content_type() {
        use anvil_bellows::{CodecRef, ParamKind, ReturnKind as RK, Route, SchemaRef, TypedParam};
        use std::path::PathBuf;

        let ctrl = Controller {
            class_name: "WebhooksController".into(),
            ctor_params: vec![],
            tags: vec![],
            security: vec![],
            routes: vec![Route {
                method: HttpMethod::Post,
                path: "/webhooks/gather".into(),
                handler_name: "gather".into(),
                is_sse: false,
                params: vec![TypedParam {
                    name: "body".into(),
                    kind: ParamKind::Consumes {
                        schema: SchemaRef {
                            ident: "GatherCallbackSchema".into(),
                        },
                        codec: CodecRef {
                            ident: "twimlRequestCodec".into(),
                            content_type: Some("application/xml".into()),
                        },
                    },
                }],
                return_kind: RK::Void { is_async: false },
                deprecated: false,
                extra_responses: vec![],
                middleware: vec![],
                authn: vec![],
                authz: vec![],
            }],
        };
        let files = vec![ControllerFile {
            source_path: PathBuf::from("/project/src/webhooks-controller.ts"),
            controllers: vec![ctrl],
        }];

        let config = OpenApiConfig {
            info: crate::config::InfoConfig {
                title: "API".into(),
                version: "1.0.0".into(),
            },
            servers: vec![],
            security_schemes: BTreeMap::new(),
        };

        let mut diagnostics = Vec::new();
        let doc = build_openapi(&files, &config, &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let op = &doc["paths"]["/webhooks/gather"]["post"];
        assert_eq!(
            op["requestBody"]["content"]["application/xml"]["schema"]["$ref"],
            json!("#/components/schemas/GatherCallbackSchema")
        );
        assert!(op["requestBody"]["content"]["application/json"].is_null());
    }

    #[test]
    fn consumes_with_unresolved_codec_defaults_and_diagnoses() {
        use anvil_bellows::{CodecRef, ParamKind, ReturnKind as RK, Route, SchemaRef, TypedParam};
        use std::path::PathBuf;

        let ctrl = Controller {
            class_name: "WebhooksController".into(),
            ctor_params: vec![],
            tags: vec![],
            security: vec![],
            routes: vec![Route {
                method: HttpMethod::Post,
                path: "/webhooks/gather".into(),
                handler_name: "gather".into(),
                is_sse: false,
                params: vec![TypedParam {
                    name: "body".into(),
                    kind: ParamKind::Consumes {
                        schema: SchemaRef {
                            ident: "GatherCallbackSchema".into(),
                        },
                        codec: CodecRef {
                            ident: "importedCodec".into(),
                            content_type: None,
                        },
                    },
                }],
                return_kind: RK::Void { is_async: false },
                deprecated: false,
                extra_responses: vec![],
                middleware: vec![],
                authn: vec![],
                authz: vec![],
            }],
        };
        let files = vec![ControllerFile {
            source_path: PathBuf::from("/project/src/webhooks-controller.ts"),
            controllers: vec![ctrl],
        }];

        let config = OpenApiConfig {
            info: crate::config::InfoConfig {
                title: "API".into(),
                version: "1.0.0".into(),
            },
            servers: vec![],
            security_schemes: BTreeMap::new(),
        };

        let mut diagnostics = Vec::new();
        let doc = build_openapi(&files, &config, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("importedCodec"));

        let op = &doc["paths"]["/webhooks/gather"]["post"];
        assert_eq!(
            op["requestBody"]["content"]["application/octet-stream"]["schema"]["$ref"],
            json!("#/components/schemas/GatherCallbackSchema")
        );
    }

    #[test]
    fn produces_emits_codecs_resolved_content_type() {
        use anvil_bellows::{CodecRef, ReturnKind as RK, Route, SchemaRef};
        use std::path::PathBuf;

        let ctrl = Controller {
            class_name: "WebhooksController".into(),
            ctor_params: vec![],
            tags: vec![],
            security: vec![],
            routes: vec![Route {
                method: HttpMethod::Post,
                path: "/webhooks/gather".into(),
                handler_name: "gather".into(),
                is_sse: false,
                params: vec![],
                return_kind: RK::Responds {
                    schema: SchemaRef {
                        ident: "TwimlResponseSchema".into(),
                    },
                    codec: Some(CodecRef {
                        ident: "twimlCodec".into(),
                        content_type: Some("application/xml".into()),
                    }),
                    is_async: false,
                },
                deprecated: false,
                extra_responses: vec![],
                middleware: vec![],
                authn: vec![],
                authz: vec![],
            }],
        };
        let files = vec![ControllerFile {
            source_path: PathBuf::from("/project/src/webhooks-controller.ts"),
            controllers: vec![ctrl],
        }];

        let config = OpenApiConfig {
            info: crate::config::InfoConfig {
                title: "API".into(),
                version: "1.0.0".into(),
            },
            servers: vec![],
            security_schemes: BTreeMap::new(),
        };

        let mut diagnostics = Vec::new();
        let doc = build_openapi(&files, &config, &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let content = &doc["paths"]["/webhooks/gather"]["post"]["responses"]["200"]["content"];
        assert_eq!(
            content["application/xml"]["schema"]["$ref"],
            json!("#/components/schemas/TwimlResponseSchema")
        );
        assert!(content["application/json"].is_null());
    }

    #[test]
    fn sse_route_gets_event_stream_content_type() {
        use crate::config::InfoConfig;
        use anvil_bellows::{ReturnKind as RK, Route};
        use std::path::PathBuf;

        let ctrl = Controller {
            class_name: "EventsController".into(),
            ctor_params: vec![],
            tags: vec![],
            security: vec![],
            routes: vec![Route {
                method: HttpMethod::Get,
                path: "/events/progress".into(),
                handler_name: "progress".into(),
                is_sse: true,
                params: vec![],
                return_kind: RK::Void { is_async: true },
                deprecated: false,
                extra_responses: vec![],
                middleware: vec![],
                authn: vec![],
                authz: vec![],
            }],
        };
        let files = vec![ControllerFile {
            source_path: PathBuf::from("/project/src/events-controller.ts"),
            controllers: vec![ctrl],
        }];

        let config = OpenApiConfig {
            info: InfoConfig {
                title: "API".into(),
                version: "1.0.0".into(),
            },
            servers: vec![],
            security_schemes: BTreeMap::new(),
        };

        let mut diagnostics = Vec::new();
        let doc = build_openapi(&files, &config, &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let content = &doc["paths"]["/events/progress"]["get"]["responses"]["200"]["content"];
        assert_eq!(
            content["text/event-stream"]["schema"]["type"],
            json!("string")
        );
    }
}
