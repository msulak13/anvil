//! Parse TypeScript controller files for `@Controller`, `@Get`, `@Post`, etc.
//!
//! Static mode only — accepts string literal decorator arguments. Non-literal
//! arguments emit a [`ParseDiagnostic`] and skip the affected route.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, ClassElement, Declaration, Decorator, Expression, FormalParameter,
    ImportDeclarationSpecifier, MethodDefinitionKind, ObjectPropertyKind, PropertyKey, Statement,
    TSLiteral, TSType, TSTypeName, TSTypeQueryExprName,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// An HTTP method extracted from a route decorator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    /// `@Get`
    Get,
    /// `@Post`
    Post,
    /// `@Put`
    Put,
    /// `@Delete`
    Delete,
    /// `@Patch`
    Patch,
}

impl HttpMethod {
    /// The decorator name that produces this HTTP method.
    #[must_use]
    pub fn decorator_name(&self) -> &'static str {
        match self {
            Self::Get => "Get",
            Self::Post => "Post",
            Self::Put => "Put",
            Self::Delete => "Delete",
            Self::Patch => "Patch",
        }
    }

    /// The HTTP verb string for the generated `RouteDefinition`.
    #[must_use]
    pub fn http_verb(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
        }
    }

    fn from_decorator_name(name: &str) -> Option<Self> {
        match name {
            "Get" => Some(Self::Get),
            "Post" => Some(Self::Post),
            "Put" => Some(Self::Put),
            "Delete" => Some(Self::Delete),
            "Patch" => Some(Self::Patch),
            _ => None,
        }
    }
}

/// A schema validator identifier (the `S` in `Body<typeof S>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaRef {
    /// The identifier used to call `.safeParse()` at runtime.
    pub ident: String,
}

/// A `ResponseCodec`/`RequestCodec` identifier (the `C` in `Produces<typeof
/// S, typeof C>` or the two-arg `Body<typeof S, typeof C>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecRef {
    /// The identifier used to read `.contentType`/call `.encode()`/
    /// `.decode()` at runtime.
    pub ident: String,
    /// The codec's `contentType` string literal, resolved statically when
    /// the codec is declared (or re-exported) as a top-level `const` object
    /// literal in the same file — used only for `OpenAPI` generation.
    /// Runtime codegen never needs this: it emits `{ident}.contentType` as a
    /// JS expression and reads it at request time instead. `None` when the
    /// codec is imported from elsewhere or its `contentType` isn't a plain
    /// string-literal property.
    pub content_type: Option<String>,
}

/// Classification of a handler parameter's type annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKind {
    /// `Body<typeof S>` — validate `req.body` against schema `S`.
    Body(SchemaRef),
    /// `Query<typeof S>` — validate `req.query` against schema `S`.
    Query(SchemaRef),
    /// `Params<typeof S>` — validate `req.params` against schema `S`.
    Params(SchemaRef),
    /// `FormBody<typeof S>` — validate `req.body` (parsed as
    /// `application/x-www-form-urlencoded`) against schema `S`.
    FormBody(SchemaRef),
    /// `Headers<typeof S>` — validate `req.headers` against schema `S`.
    Headers(SchemaRef),
    /// The two-arg `Body<typeof S, typeof C>` — decode `req.body` with
    /// `RequestCodec` `C` (whose `contentType` scopes which requests it
    /// applies to) before validating the decoded value against schema `S`.
    Consumes {
        /// Schema used to validate the codec's decoded value.
        schema: SchemaRef,
        /// The `RequestCodec` that decodes the raw request body.
        codec: CodecRef,
    },
    /// `RawBody` — inject the raw, unparsed request body bytes (`req.rawBody`).
    RawBody,
    /// `Request` or `express.Request` — inject the raw request object.
    Request,
    /// `Response` or `express.Response` — inject the raw response object.
    Response,
    /// `AuthnUser<T>` — inject the identified user (`res.locals.user`),
    /// validated against the route's `@Authn` services' declared user type.
    User(UserTypeRef),
    /// `SseStream` — inject an `SseStream` wrapping `res`, for `@Sse` routes.
    Sse,
    /// `AbortSignal` — inject a signal that fires on client disconnect.
    Signal,
    /// Unrecognized annotation — triggers v0.1 passthrough for the whole route.
    Unknown,
}

/// The resolved declaration site of a type — used to compare `AuthnUser<T>`
/// against the `U` each `@Authn` service declares in
/// `implements AuthnService<U, Scheme>`, by identity rather than by name
/// alone (two different `User` types in different files must not compare
/// equal just because they share a name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeIdentity {
    /// Absolute path to the file that declares (or, if imported, re-exports)
    /// the type at its declaration site.
    pub file: PathBuf,
    /// The type's local name at that declaration site.
    pub name: String,
}

/// The `T` in a handler's `AuthnUser<T>` parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserTypeRef {
    /// Raw type text, for diagnostics — the identifier name, or `"<inline>"`
    /// when `T` isn't a bare type reference.
    pub type_name: String,
    /// The resolved declaration site, or `None` when `T` isn't a bare
    /// identifier type reference (e.g. an inline object type) and so can't
    /// be identity-compared against `@Authn` services' user types.
    pub identity: Option<TypeIdentity>,
}

/// A named parameter extracted from a handler method signature.
#[derive(Debug, Clone)]
pub struct TypedParam {
    /// The parameter's declared name in TypeScript.
    pub name: String,
    /// The resolved kind.
    pub kind: ParamKind,
}

/// Classification of a handler method's return type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnKind {
    /// `Responds<typeof S>`, `Produces<typeof S, typeof C>`, or a `Promise<…>` of either.
    Responds {
        /// Schema used to validate the return value before serializing it.
        schema: SchemaRef,
        /// From `Produces<typeof S, typeof C>` — a `ResponseCodec` identifier
        /// that serializes the validated value instead of the default
        /// `res.json()`. `None` (the `Responds<typeof S>` form) keeps today's
        /// JSON behavior.
        codec: Option<CodecRef>,
        /// True when the method is `async` or returns `Promise<…>`.
        is_async: bool,
    },
    /// `void`, `Promise<void>`, or no annotation — controller owns `res`.
    Void {
        /// True when the method is `async`.
        is_async: bool,
    },
}

/// An additional HTTP response declared via `@Returns(status, schema)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraResponse {
    /// HTTP status code.
    pub status: u16,
    /// Optional schema identifier (from the second argument).
    pub schema: Option<SchemaRef>,
}

/// A single route extracted from a method decorator.
#[derive(Debug, Clone)]
pub struct Route {
    /// HTTP method.
    pub method: HttpMethod,
    /// The full path: `base_path` joined with the route decorator path.
    pub path: String,
    /// The TypeScript method name on the controller class.
    pub handler_name: String,
    /// True when declared via `@Sse` rather than `@Get`/`@Post`/etc. — the
    /// route is registered as `GET` but marked streaming for codegen/OpenAPI.
    pub is_sse: bool,
    /// Typed parameters of the handler (empty means v0.1 passthrough).
    pub params: Vec<TypedParam>,
    /// Classification of the handler's return type.
    pub return_kind: ReturnKind,
    /// True when `@Deprecated` is present on the method.
    pub deprecated: bool,
    /// Additional responses from `@Returns(status, schema)` decorators.
    pub extra_responses: Vec<ExtraResponse>,
    /// Ordered middleware chain from `@Middleware(...)` — class-level entries
    /// first, then method-level, in source order.
    pub middleware: Vec<MiddlewareRef>,
    /// Ordered authentication cascade from `@Authn(...)` — class-level entries
    /// first, then method-level, in source order.
    pub authn: Vec<AuthRef>,
    /// Ordered authorization cascade from `@Authz(...)` — class-level entries
    /// first, then method-level, in source order.
    pub authz: Vec<AuthRef>,
}

/// Where an imported identifier comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOrigin {
    /// Resolved absolute path from a relative import specifier (`"./foo"`).
    Relative(PathBuf),
    /// A bare package specifier (e.g. `"express-rate-limit"`).
    Package(String),
}

/// A middleware function referenced via `@Middleware(fn)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiddlewareRef {
    /// The identifier used to reference the middleware function.
    pub name: String,
    /// Where `name` is imported from, if it could be resolved from an
    /// import statement in the controller file. `None` means it's assumed
    /// to be declared or re-exported by the controller file itself.
    pub origin: Option<ImportOrigin>,
}

/// A service class referenced via `@Authn(...)` or `@Authz(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthRef {
    /// The class name used to reference the service.
    pub name: String,
    /// Where `name` is imported from, if it could be resolved from an
    /// import statement in the controller file. `None` means it's assumed
    /// to be declared or re-exported by the controller file itself.
    pub origin: Option<ImportOrigin>,
    /// For `@Authn` refs only: the `OpenAPI` security-scheme key read directly
    /// off the class's `implements AuthnService<U, "scheme">` clause. `None`
    /// when the class can't be resolved (e.g. a package import) or the
    /// literal isn't present in that exact shape — this only affects `OpenAPI`
    /// visibility, never the runtime auth check. Always `None` for `@Authz`.
    pub scheme: Option<String>,
    /// For `@Authn` refs only: the resolved declaration site of `U` in
    /// `implements AuthnService<U, Scheme>`, used to validate `AuthnUser<T>`
    /// handler parameters. `None` when unresolvable (same conditions as
    /// `scheme`, plus `U` not being a bare identifier type reference).
    /// Always `None` for `@Authz`.
    pub user_identity: Option<TypeIdentity>,
}

/// A single constructor parameter on a `@Controller` class.
#[derive(Debug, Clone)]
pub struct CtorParam {
    /// The parameter's local name (e.g. `todoService`).
    pub name: String,
    /// The TypeScript type name (e.g. `TodoService`).
    pub type_name: String,
    /// Where `type_name` is imported from, if it could be resolved from an
    /// import statement in the controller file (relative source path, or a
    /// bare/scoped package specifier — same as `MiddlewareRef`/`AuthRef`).
    /// `None` when the import is unresolvable or the type is declared in the
    /// controller file itself.
    pub origin: Option<ImportOrigin>,
}

/// A controller class extracted from a source file.
#[derive(Debug, Clone)]
pub struct Controller {
    /// TypeScript class name.
    pub class_name: String,
    /// Constructor parameters (the controller's DI dependencies).
    pub ctor_params: Vec<CtorParam>,
    /// Routes declared on this controller.
    pub routes: Vec<Route>,
    /// Tags from `@Tag("name")` class decorators.
    pub tags: Vec<String>,
    /// Security scheme names from `@Security("scheme")` class decorators.
    pub security: Vec<String>,
}

/// A parsed controller file containing one or more `@Controller` classes.
#[derive(Debug, Clone)]
pub struct ControllerFile {
    /// Absolute path to the source file.
    pub source_path: PathBuf,
    /// Controllers found in this file.
    pub controllers: Vec<Controller>,
}

/// A non-fatal diagnostic emitted when a decorator argument is not a string literal.
#[derive(Debug, Clone)]
pub struct ParseDiagnostic {
    /// Source file path.
    pub file: String,
    /// Decorator name (`Controller`, `Get`, etc.).
    pub decorator: String,
    /// Description of what was found instead of a literal.
    pub found: String,
    /// User-facing hint.
    pub hint: String,
}

impl ParseDiagnostic {
    /// Render as the user-facing error string.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "Error [anvil-bellows]: @{} argument is not a string literal.\n  Found {}\n  Hint: {}\n  → {}",
            self.decorator, self.found, self.hint, self.file,
        )
    }
}

/// Parse all `.ts` files under `entry_dir` and return controller metadata.
///
/// Files with no `@Controller` decorator are silently skipped.
/// Non-literal decorator arguments emit a [`ParseDiagnostic`] and skip that route.
///
/// # Errors
///
/// Returns an error if glob expansion or file I/O fails.
pub fn parse_entry(
    entry_dir: &Path,
) -> Result<(Vec<ControllerFile>, Vec<ParseDiagnostic>), anyhow::Error> {
    let pattern = entry_dir.join("**/*.ts").to_string_lossy().into_owned();

    let mut files = Vec::new();
    let mut diagnostics = Vec::new();

    for entry in glob::glob(&pattern)? {
        let path = entry?;
        // Canonicalize so that macOS symlinks (/tmp → /private/tmp) don't
        // break relative-path computation against a canonicalized entry_dir.
        let path = std::fs::canonicalize(&path).unwrap_or(path);

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        // Skip generated files, declarations, and tests.
        if name.ends_with(".d.ts") || name.ends_with(".test.ts") || name == "routes.module.ts" {
            continue;
        }

        let source = std::fs::read_to_string(&path)?;
        let (file_opt, mut diags) = parse_source(&source, &path.display().to_string(), &path);
        diagnostics.append(&mut diags);
        if let Some(f) = file_opt {
            files.push(f);
        }
    }

    // Sort by path for deterministic output.
    files.sort_by(|a, b| a.source_path.cmp(&b.source_path));

    Ok((files, diagnostics))
}

/// Parse a single TypeScript source string.
#[allow(clippy::too_many_lines, clippy::similar_names)]
fn parse_source(
    source: &str,
    file_path: &str,
    abs_path: &Path,
) -> (Option<ControllerFile>, Vec<ParseDiagnostic>) {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    if !ret.errors.is_empty() {
        // Skip files with syntax errors; they will fail tsc anyway.
        return (None, vec![]);
    }

    // Build a map of local name → absolute source path for relative imports.
    // Used to resolve constructor dep types to importable paths.
    let file_dir = abs_path.parent().unwrap_or(Path::new("."));
    let import_map = collect_import_map(&ret.program.body, file_dir);
    let codec_literals = collect_content_type_literals(&ret.program.body);

    let mut controllers: Vec<Controller> = Vec::new();
    let mut diagnostics: Vec<ParseDiagnostic> = Vec::new();

    for stmt in &ret.program.body {
        let class = match stmt {
            Statement::ClassDeclaration(c) => c.as_ref(),
            Statement::ExportNamedDeclaration(decl) => match &decl.declaration {
                Some(Declaration::ClassDeclaration(c)) => c.as_ref(),
                _ => continue,
            },
            _ => continue,
        };

        let Some(class_ident) = &class.id else {
            continue;
        };
        let class_name = class_ident.name.to_string();

        let Some(base_path) =
            extract_controller_path(&class.decorators, file_path, &mut diagnostics)
        else {
            continue;
        };

        let class_meta =
            extract_class_metadata(&class.decorators, &import_map, file_path, &mut diagnostics);
        let mut routes: Vec<Route> = Vec::new();
        let mut ctor_params: Vec<CtorParam> = Vec::new();

        for element in &class.body.body {
            let ClassElement::MethodDefinition(m) = element else {
                continue;
            };
            if m.kind == MethodDefinitionKind::Constructor {
                // Extract the controller's DI dependencies from its ctor params.
                ctor_params = m
                    .value
                    .params
                    .items
                    .iter()
                    .filter_map(|p| extract_ctor_param(p, &import_map))
                    .collect();
                continue;
            }

            let handler_name = match &m.key {
                PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                PropertyKey::StringLiteral(s) => s.value.to_string(),
                _ => continue,
            };

            let typed_params: Vec<TypedParam> = m
                .value
                .params
                .items
                .iter()
                .map(|p| classify_param(p, &import_map, &codec_literals, abs_path))
                .collect();

            let return_kind = classify_return(
                m.value.return_type.as_ref().map(|a| &a.type_annotation),
                m.value.r#async,
                &codec_literals,
            );

            // Scan all method decorators: find HTTP method + collect modifiers.
            let mut route_opt: Option<Route> = None;
            let mut deprecated = false;
            let mut extra_responses: Vec<ExtraResponse> = Vec::new();
            let mut method_middleware: Vec<MiddlewareRef> = Vec::new();
            let mut method_authn: Vec<AuthRef> = Vec::new();
            let mut method_authz: Vec<AuthRef> = Vec::new();

            for dec in &m.decorators {
                if let Some(route) = try_extract_route(
                    dec,
                    &base_path,
                    &handler_name,
                    &typed_params,
                    return_kind.clone(),
                    file_path,
                    &mut diagnostics,
                ) {
                    route_opt = Some(route);
                } else if is_deprecated_decorator(dec) {
                    deprecated = true;
                } else if let Some(er) = try_extract_returns(dec) {
                    extra_responses.push(er);
                } else if let Some(mut mw) =
                    try_extract_middleware(dec, &import_map, file_path, &mut diagnostics)
                {
                    method_middleware.append(&mut mw);
                } else if let Some(mut refs) =
                    try_extract_auth_refs(dec, "Authn", &import_map, file_path, &mut diagnostics)
                {
                    method_authn.append(&mut refs);
                } else if let Some(mut refs) =
                    try_extract_auth_refs(dec, "Authz", &import_map, file_path, &mut diagnostics)
                {
                    method_authz.append(&mut refs);
                }
            }

            if let Some(mut route) = route_opt {
                route.deprecated = deprecated;
                route.extra_responses = extra_responses;
                route.middleware = class_meta
                    .middleware
                    .iter()
                    .cloned()
                    .chain(method_middleware)
                    .collect();
                route.authn = class_meta
                    .authn
                    .iter()
                    .cloned()
                    .chain(method_authn)
                    .collect();
                route.authz = class_meta
                    .authz
                    .iter()
                    .cloned()
                    .chain(method_authz)
                    .collect();
                for a in &mut route.authn {
                    let resolved = resolve_authn_type_args(&a.name, a.origin.as_ref());
                    a.scheme = resolved.scheme;
                    a.user_identity = resolved.user_identity;
                }
                if let Some(diag) = validate_sse_route(&route, file_path) {
                    diagnostics.push(diag);
                } else if let Some(diag) = validate_authn_user_param(&route, file_path) {
                    diagnostics.push(diag);
                } else {
                    routes.push(route);
                }
            }
        }

        if !routes.is_empty() {
            controllers.push(Controller {
                class_name,
                ctor_params,
                routes,
                tags: class_meta.tags,
                security: class_meta.security,
            });
        }
    }

    if controllers.is_empty() {
        return (None, diagnostics);
    }

    (
        Some(ControllerFile {
            source_path: abs_path.to_path_buf(),
            controllers,
        }),
        diagnostics,
    )
}

/// Statically resolve each top-level `const <ident> = { ..., contentType:
/// "<literal>", ... }` declaration in `stmts` (bare or `export`ed) into
/// `ident -> literal`. Used to recover a `ResponseCodec`/`RequestCodec`'s
/// `contentType` for `OpenAPI` generation — `Produces`/the two-arg `Body<S,
/// C>` only capture the codec's identifier at the type level, not its
/// runtime value.
/// Codecs declared outside the controller file (only imported) aren't
/// resolved this way; `OpenAPI` generation falls back to a diagnostic
/// default in that case. This doesn't affect runtime codegen, which mounts
/// `{ident}.contentType` as a JS expression and reads it at request time.
fn collect_content_type_literals(stmts: &[Statement<'_>]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for stmt in stmts {
        let decl = match stmt {
            Statement::VariableDeclaration(d) => Some(d.as_ref()),
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(d)) => Some(d.as_ref()),
                _ => None,
            },
            _ => None,
        };
        let Some(decl) = decl else {
            continue;
        };
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            let Some(Expression::ObjectExpression(obj)) = &declarator.init else {
                continue;
            };
            for prop in &obj.properties {
                let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                    continue;
                };
                if property_key_name(&prop.key) != Some("contentType") {
                    continue;
                }
                if let Expression::StringLiteral(lit) = &prop.value {
                    out.insert(id.name.to_string(), lit.value.to_string());
                }
            }
        }
    }
    out
}

/// The static name of an object property key, when it's a plain identifier
/// or string literal (not computed).
fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

/// Walk import declarations in `stmts` and build a map from local name to its
/// import origin (relative source path, or bare package specifier).
fn collect_import_map(stmts: &[Statement<'_>], file_dir: &Path) -> HashMap<String, ImportOrigin> {
    let mut map: HashMap<String, ImportOrigin> = HashMap::new();
    for stmt in stmts {
        let Statement::ImportDeclaration(import) = stmt else {
            continue;
        };
        let raw_spec = import.source.value.as_str();
        let origin = if raw_spec.starts_with("./") || raw_spec.starts_with("../") {
            // Strip .js / .ts extension for path resolution then add .ts.
            let stem = raw_spec
                .strip_suffix(".js")
                .or_else(|| raw_spec.strip_suffix(".ts"))
                .unwrap_or(raw_spec);
            ImportOrigin::Relative(file_dir.join(format!("{stem}.ts")))
        } else {
            ImportOrigin::Package(raw_spec.to_owned())
        };
        let Some(specifiers) = &import.specifiers else {
            continue;
        };
        for spec in specifiers {
            let local_name = match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => s.local.name.as_str().to_owned(),
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    s.local.name.as_str().to_owned()
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    s.local.name.as_str().to_owned()
                }
            };
            map.insert(local_name, origin.clone());
        }
    }
    map
}

/// Extract a `CtorParam` from a formal parameter.
///
/// Only plain class-type params (bare `TSTypeReference` with a simple identifier)
/// are recognized. Wrapper types like `Body<S>` and primitives are skipped.
fn extract_ctor_param(
    param: &FormalParameter<'_>,
    import_map: &HashMap<String, ImportOrigin>,
) -> Option<CtorParam> {
    let name = match &param.pattern {
        BindingPattern::BindingIdentifier(id) => id.name.as_str().to_owned(),
        _ => return None,
    };
    let ann = param.type_annotation.as_ref()?;
    let TSType::TSTypeReference(tref) = &ann.type_annotation else {
        return None;
    };
    let type_name = ts_type_name_local(&tref.type_name);
    // Skip known wrapper types — they're not injected class deps.
    match type_name {
        "Body" | "Query" | "Params" | "Responds" | "Request" | "Response" | "FormBody"
        | "Headers" | "RawBody" => return None,
        _ => {}
    }
    let type_name = type_name.to_owned();
    let origin = import_map.get(&type_name).cloned();
    Some(CtorParam {
        name,
        type_name,
        origin,
    })
}

/// Extract the path string from `@Controller(path)` in a class's decorator list.
fn extract_controller_path<'a>(
    decorators: &'a [Decorator<'a>],
    file_path: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Option<String> {
    for dec in decorators {
        let Expression::CallExpression(call) = &dec.expression else {
            continue;
        };
        let name = decorator_ident_name(&call.callee)?;
        if name != "Controller" {
            continue;
        }
        let arg = call.arguments.first()?;
        return extract_string_arg(arg, "Controller", file_path, diagnostics);
    }
    None
}

/// Try to extract a route from a single method decorator (`@Get`, `@Post`, etc.).
fn try_extract_route<'a>(
    dec: &'a Decorator<'a>,
    base_path: &str,
    handler_name: &str,
    typed_params: &[TypedParam],
    return_kind: ReturnKind,
    file_path: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Option<Route> {
    let Expression::CallExpression(call) = &dec.expression else {
        return None;
    };
    let name = decorator_ident_name(&call.callee)?;
    let (method, is_sse) = if name == "Sse" {
        (HttpMethod::Get, true)
    } else {
        (HttpMethod::from_decorator_name(&name)?, false)
    };
    let arg = call.arguments.first()?;
    let route_path = extract_string_arg(arg, &name, file_path, diagnostics)?;
    let full_path = join_paths(base_path, &route_path);
    Some(Route {
        method,
        path: full_path,
        handler_name: handler_name.to_owned(),
        is_sse,
        params: typed_params.to_vec(),
        return_kind,
        deprecated: false, // filled in by caller after scanning all decorators
        extra_responses: vec![], // ditto
        middleware: vec![], // ditto
        authn: vec![],     // ditto
        authz: vec![],     // ditto
    })
}

/// Metadata collected from a controller class's own decorators (as opposed to
/// its methods').
struct ClassMetadata {
    tags: Vec<String>,
    security: Vec<String>,
    middleware: Vec<MiddlewareRef>,
    authn: Vec<AuthRef>,
    authz: Vec<AuthRef>,
}

/// Extract `@Tag("name")`, `@Security("scheme")`, `@Middleware(...)`,
/// `@Authn(...)`, and `@Authz(...)` from class decorators.
#[allow(clippy::similar_names)]
fn extract_class_metadata(
    decorators: &[Decorator<'_>],
    import_map: &HashMap<String, ImportOrigin>,
    file_path: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> ClassMetadata {
    let mut tags = Vec::new();
    let mut security = Vec::new();
    let mut middleware = Vec::new();
    let mut authn = Vec::new();
    let mut authz = Vec::new();
    for dec in decorators {
        if let Some(mut refs) = try_extract_middleware(dec, import_map, file_path, diagnostics) {
            middleware.append(&mut refs);
            continue;
        }
        if let Some(mut refs) =
            try_extract_auth_refs(dec, "Authn", import_map, file_path, diagnostics)
        {
            authn.append(&mut refs);
            continue;
        }
        if let Some(mut refs) =
            try_extract_auth_refs(dec, "Authz", import_map, file_path, diagnostics)
        {
            authz.append(&mut refs);
            continue;
        }
        let Expression::CallExpression(call) = &dec.expression else {
            continue;
        };
        let Some(name) = decorator_ident_name(&call.callee) else {
            continue;
        };
        let Some(arg) = call.arguments.first() else {
            continue;
        };
        match name.as_str() {
            "Tag" => {
                if let Argument::StringLiteral(lit) = arg {
                    tags.push(lit.value.to_string());
                }
            }
            "Security" => {
                if let Argument::StringLiteral(lit) = arg {
                    security.push(lit.value.to_string());
                }
            }
            _ => {}
        }
    }
    ClassMetadata {
        tags,
        security,
        middleware,
        authn,
        authz,
    }
}

/// Try to extract a `@Middleware(fn1, fn2, ...)` decorator's identifier
/// arguments, resolving each to its import origin when possible.
fn try_extract_middleware(
    dec: &Decorator<'_>,
    import_map: &HashMap<String, ImportOrigin>,
    file_path: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Option<Vec<MiddlewareRef>> {
    let Expression::CallExpression(call) = &dec.expression else {
        return None;
    };
    if decorator_ident_name(&call.callee).as_deref() != Some("Middleware") {
        return None;
    }
    let mut refs = Vec::new();
    for arg in &call.arguments {
        if let Argument::Identifier(id) = arg {
            let name = id.name.to_string();
            let origin = import_map.get(&name).cloned();
            refs.push(MiddlewareRef { name, origin });
        } else {
            diagnostics.push(ParseDiagnostic {
                file: file_path.to_owned(),
                decorator: "Middleware".to_owned(),
                found: describe_arg(arg),
                hint: "use a bare identifier imported (or declared) at module scope".to_owned(),
            });
        }
    }
    Some(refs)
}

/// Try to extract a `@Authn(Class1, Class2, ...)` or `@Authz(...)` decorator's
/// identifier arguments, resolving each to its import origin when possible.
/// `decorator_name` is `"Authn"` or `"Authz"`; the returned refs' `scheme` is
/// always `None` here — callers resolve it separately for `@Authn` refs.
fn try_extract_auth_refs(
    dec: &Decorator<'_>,
    decorator_name: &str,
    import_map: &HashMap<String, ImportOrigin>,
    file_path: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Option<Vec<AuthRef>> {
    let Expression::CallExpression(call) = &dec.expression else {
        return None;
    };
    if decorator_ident_name(&call.callee).as_deref() != Some(decorator_name) {
        return None;
    }
    let mut refs = Vec::new();
    for arg in &call.arguments {
        if let Argument::Identifier(id) = arg {
            let name = id.name.to_string();
            let origin = import_map.get(&name).cloned();
            refs.push(AuthRef {
                name,
                origin,
                scheme: None,
                user_identity: None,
            });
        } else {
            diagnostics.push(ParseDiagnostic {
                file: file_path.to_owned(),
                decorator: decorator_name.to_owned(),
                found: describe_arg(arg),
                hint: "use a bare identifier imported (or declared) at module scope".to_owned(),
            });
        }
    }
    Some(refs)
}

/// The `U` and `Scheme` type arguments read off a class's
/// `implements AuthnService<U, Scheme>` clause.
struct AuthnTypeArgs {
    user_identity: Option<TypeIdentity>,
    scheme: Option<String>,
}

/// Resolve `type_name`'s declaration site: if it's imported via a relative
/// import in `import_map`, that import's target file; if it's a package
/// import, unresolvable (`None`); otherwise assumed declared in `file_path`
/// itself (mirrors the same-file convention used for middleware/auth refs).
fn resolve_type_identity(
    type_name: &str,
    import_map: &HashMap<String, ImportOrigin>,
    file_path: &Path,
) -> Option<TypeIdentity> {
    match import_map.get(type_name) {
        Some(ImportOrigin::Relative(p)) => Some(TypeIdentity {
            file: p.clone(),
            name: type_name.to_owned(),
        }),
        Some(ImportOrigin::Package(_)) => None,
        None => Some(TypeIdentity {
            file: file_path.to_path_buf(),
            name: type_name.to_owned(),
        }),
    }
}

/// Resolve the `U` (user type) and `Scheme` (`OpenAPI` security-scheme key)
/// type arguments from `class_name`'s `implements AuthnService<U, "scheme">`
/// clause, by opening and parsing the file it's declared in. Both resolve to
/// `None` independently when `origin` isn't a resolvable relative import, the
/// class or clause can't be found, the target file has syntax errors, or the
/// respective type argument isn't in the exact required shape (a bare
/// identifier type reference for `U`, a string-literal type for `Scheme`) —
/// any failure here is silent; callers turn `None` into diagnostics only
/// where actually required (`AuthnUser<T>` validation), since `Scheme` alone
/// is optional `OpenAPI` metadata.
fn resolve_authn_type_args(class_name: &str, origin: Option<&ImportOrigin>) -> AuthnTypeArgs {
    let unresolved = AuthnTypeArgs {
        user_identity: None,
        scheme: None,
    };
    let Some(ImportOrigin::Relative(path)) = origin else {
        return unresolved;
    };
    let Ok(source) = std::fs::read_to_string(path) else {
        return unresolved;
    };
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source, SourceType::ts()).parse();
    if !ret.errors.is_empty() {
        return unresolved;
    }
    let file_dir = path.parent().unwrap_or(Path::new("."));
    let service_import_map = collect_import_map(&ret.program.body, file_dir);

    for stmt in &ret.program.body {
        let class = match stmt {
            Statement::ClassDeclaration(c) => c.as_ref(),
            Statement::ExportNamedDeclaration(decl) => match &decl.declaration {
                Some(Declaration::ClassDeclaration(c)) => c.as_ref(),
                _ => continue,
            },
            _ => continue,
        };
        let Some(id) = &class.id else { continue };
        if id.name.as_str() != class_name {
            continue;
        }
        for implements in &class.implements {
            if ts_type_name_local(&implements.expression) != "AuthnService" {
                continue;
            }
            let Some(type_args) = implements.type_arguments.as_ref() else {
                return unresolved;
            };
            let user_identity = type_args.params.first().and_then(|t| {
                let TSType::TSTypeReference(tref) = t else {
                    return None;
                };
                if !matches!(tref.type_name, TSTypeName::IdentifierReference(_)) {
                    return None;
                }
                let name = ts_type_name_local(&tref.type_name);
                resolve_type_identity(name, &service_import_map, path)
            });
            let scheme = type_args.params.get(1).and_then(|t| {
                let TSType::TSLiteralType(lit) = t else {
                    return None;
                };
                let TSLiteral::StringLiteral(s) = &lit.literal else {
                    return None;
                };
                Some(s.value.to_string())
            });
            return AuthnTypeArgs {
                user_identity,
                scheme,
            };
        }
    }
    unresolved
}

/// Validate `AuthnUser<T>` handler parameters against the route's `@Authn`
/// services, requiring `T` to identity-match the user type declared by
/// *every* service on the route (the "matches all of" rule) — the most
/// restrictive option, chosen because a route whose declared services
/// disagree (or can't be proven to agree) would otherwise resolve to a
/// non-deterministic user shape at runtime, undetectable by the type system.
/// Returns `Some(diagnostic)` and the caller drops the route entirely; `None`
/// means either no `AuthnUser<T>` param is present, or it's provably safe.
fn validate_authn_user_param(route: &Route, file_path: &str) -> Option<ParseDiagnostic> {
    let user_params: Vec<&UserTypeRef> = route
        .params
        .iter()
        .filter_map(|p| match &p.kind {
            ParamKind::User(u) => Some(u),
            _ => None,
        })
        .collect();
    if user_params.is_empty() {
        return None;
    }

    if route.authn.is_empty() {
        return Some(ParseDiagnostic {
            file: file_path.to_owned(),
            decorator: "AuthnUser".to_owned(),
            found: "no @Authn services declared on this route".to_owned(),
            hint: "declare at least one @Authn(...) service, or remove the AuthnUser<T> parameter"
                .to_owned(),
        });
    }

    // Every @Authn service must resolve to the same user-type identity —
    // an unresolvable identity can't be proven consistent, so it's treated
    // the same as a disagreement.
    let mut common: Option<&TypeIdentity> = None;
    for a in &route.authn {
        let Some(id) = &a.user_identity else {
            return Some(ParseDiagnostic {
                file: file_path.to_owned(),
                decorator: "AuthnUser".to_owned(),
                found: format!(
                    "@Authn service `{}` has no directly-resolvable user type in its \
                     `implements AuthnService<U, Scheme>` clause",
                    a.name
                ),
                hint: "AuthnUser<T> requires every @Authn service on the route to declare a \
                       bare, resolvable identifier as AuthnService's first type argument"
                    .to_owned(),
            });
        };
        match common {
            None => common = Some(id),
            Some(existing) if existing != id => {
                return Some(ParseDiagnostic {
                    file: file_path.to_owned(),
                    decorator: "AuthnUser".to_owned(),
                    found: "this route's @Authn services declare different user types".to_owned(),
                    hint: "AuthnUser<T> requires all @Authn services on a route to agree on \
                           the same user type — consider a shared base type"
                        .to_owned(),
                });
            }
            Some(_) => {}
        }
    }
    let common = common.expect("route.authn is non-empty, so common was set in the loop above");

    for u in &user_params {
        let Some(t_identity) = &u.identity else {
            return Some(ParseDiagnostic {
                file: file_path.to_owned(),
                decorator: "AuthnUser".to_owned(),
                found: format!(
                    "AuthnUser<{}> is not a directly-resolvable named type",
                    u.type_name
                ),
                hint: "use a bare imported or locally-declared type identifier as AuthnUser's \
                       type argument"
                    .to_owned(),
            });
        };
        if t_identity != common {
            return Some(ParseDiagnostic {
                file: file_path.to_owned(),
                decorator: "AuthnUser".to_owned(),
                found: format!(
                    "AuthnUser<{}> does not match this route's @Authn user type",
                    u.type_name
                ),
                hint: "make AuthnUser<T>'s T identical to the user type declared by every \
                       @Authn service on this route"
                    .to_owned(),
            });
        }
    }
    None
}

/// Validate `@Sse` routes: the handler must return `void`/`Promise<void>` —
/// there's no buffered value to validate/serialize on a stream, so
/// `Responds<S>`/`Produces<S, C>` return types are rejected rather than
/// silently ignored. Returns `Some(diagnostic)` and the caller drops the
/// route; `None` means either the route isn't `@Sse` or it's valid.
fn validate_sse_route(route: &Route, file_path: &str) -> Option<ParseDiagnostic> {
    if !route.is_sse {
        return None;
    }
    if matches!(route.return_kind, ReturnKind::Void { .. }) {
        return None;
    }
    Some(ParseDiagnostic {
        file: file_path.to_owned(),
        decorator: "Sse".to_owned(),
        found: "handler declares a Responds<S>/Produces<S, C> return type".to_owned(),
        hint: "@Sse handlers must return void — write events with the injected SseStream \
               instead of returning a value"
            .to_owned(),
    })
}

/// Return true when `@Deprecated` is present (with or without arguments).
fn is_deprecated_decorator(dec: &Decorator<'_>) -> bool {
    match &dec.expression {
        Expression::CallExpression(call) => {
            decorator_ident_name(&call.callee).as_deref() == Some("Deprecated")
        }
        Expression::Identifier(id) => id.name.as_str() == "Deprecated",
        _ => false,
    }
}

/// Try to extract an `ExtraResponse` from `@Returns(status, schema?)`.
fn try_extract_returns(dec: &Decorator<'_>) -> Option<ExtraResponse> {
    let Expression::CallExpression(call) = &dec.expression else {
        return None;
    };
    if decorator_ident_name(&call.callee).as_deref() != Some("Returns") {
        return None;
    }
    let status_arg = call.arguments.first()?;
    let Argument::NumericLiteral(lit) = status_arg else {
        return None;
    };
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let status = lit.value as u16;
    let schema = call.arguments.get(1).and_then(|a| {
        if let Argument::Identifier(id) = a {
            Some(SchemaRef {
                ident: id.name.to_string(),
            })
        } else {
            None
        }
    });
    Some(ExtraResponse { status, schema })
}

/// Classify a formal parameter by its type annotation.
fn classify_param(
    param: &FormalParameter<'_>,
    import_map: &HashMap<String, ImportOrigin>,
    codec_literals: &HashMap<String, String>,
    file_path: &Path,
) -> TypedParam {
    let name = match &param.pattern {
        BindingPattern::BindingIdentifier(id) => id.name.as_str().to_owned(),
        _ => "_".to_owned(),
    };
    let kind = param
        .type_annotation
        .as_ref()
        .map_or(ParamKind::Unknown, |ann| {
            classify_ts_type(&ann.type_annotation, import_map, codec_literals, file_path)
        });
    TypedParam { name, kind }
}

/// Classify a `TSType` into a `ParamKind`.
fn classify_ts_type(
    ty: &TSType<'_>,
    import_map: &HashMap<String, ImportOrigin>,
    codec_literals: &HashMap<String, String>,
    file_path: &Path,
) -> ParamKind {
    let TSType::TSTypeReference(tref) = ty else {
        return ParamKind::Unknown;
    };
    let local_name = ts_type_name_local(&tref.type_name);
    match local_name {
        "Request" => return ParamKind::Request,
        "Response" => return ParamKind::Response,
        "RawBody" => return ParamKind::RawBody,
        "SseStream" => return ParamKind::Sse,
        "AbortSignal" => return ParamKind::Signal,
        "AuthnUser" => return classify_authn_user(tref, import_map, file_path),
        // The optional second type argument distinguishes the two-arg
        // `Body<S, C>` form (decode with a `RequestCodec` before validating)
        // from plain `Body<S>`.
        "Body" => {
            let Some(schema) = extract_typeof_at(tref, 0) else {
                return ParamKind::Unknown;
            };
            return match extract_typeof_at(tref, 1) {
                Some(codec) => ParamKind::Consumes {
                    schema,
                    codec: resolve_codec_ref(codec, codec_literals),
                },
                None => ParamKind::Body(schema),
            };
        }
        "Query" | "Params" | "FormBody" | "Headers" => {}
        _ => return ParamKind::Unknown,
    }
    let Some(schema) = extract_typeof_at(tref, 0) else {
        return ParamKind::Unknown;
    };
    match local_name {
        "Query" => ParamKind::Query(schema),
        "Params" => ParamKind::Params(schema),
        "FormBody" => ParamKind::FormBody(schema),
        "Headers" => ParamKind::Headers(schema),
        _ => unreachable!(),
    }
}

/// Resolve a `typeof C` reference into a `CodecRef`, filling in `content_type`
/// from `codec_literals` when `C`'s `contentType` property was statically
/// resolvable (see [`collect_content_type_literals`]).
fn resolve_codec_ref(codec: SchemaRef, codec_literals: &HashMap<String, String>) -> CodecRef {
    let content_type = codec_literals.get(&codec.ident).cloned();
    CodecRef {
        ident: codec.ident,
        content_type,
    }
}

/// Classify `AuthnUser<T>`, resolving `T`'s declaration site when it's a bare
/// identifier type reference (the only shape `AuthnUser<T>` validation can
/// identity-compare against `@Authn` services' declared user type).
fn classify_authn_user(
    tref: &oxc_ast::ast::TSTypeReference<'_>,
    import_map: &HashMap<String, ImportOrigin>,
    file_path: &Path,
) -> ParamKind {
    let Some(t_arg) = tref.type_arguments.as_ref().and_then(|p| p.params.first()) else {
        return ParamKind::Unknown;
    };
    let (type_name, identity) = match t_arg {
        TSType::TSTypeReference(inner)
            if matches!(inner.type_name, TSTypeName::IdentifierReference(_)) =>
        {
            let name = ts_type_name_local(&inner.type_name).to_owned();
            let identity = resolve_type_identity(&name, import_map, file_path);
            (name, identity)
        }
        _ => ("<inline>".to_owned(), None),
    };
    ParamKind::User(UserTypeRef {
        type_name,
        identity,
    })
}

/// Classify the return type annotation into a `ReturnKind`.
fn classify_return(
    ty: Option<&TSType<'_>>,
    is_async: bool,
    codec_literals: &HashMap<String, String>,
) -> ReturnKind {
    let Some(ty) = ty else {
        return ReturnKind::Void { is_async };
    };
    // Handle Promise<X> — unwrap to inner type.
    let (inner, resolved_async) = if let TSType::TSTypeReference(tref) = ty {
        if ts_type_name_local(&tref.type_name) == "Promise" {
            let inner = tref
                .type_arguments
                .as_ref()
                .and_then(|tp| tp.params.first());
            (inner.map(|t| t as &TSType<'_>), true)
        } else {
            (Some(ty), is_async)
        }
    } else {
        (Some(ty), is_async)
    };

    let Some(inner) = inner else {
        return ReturnKind::Void {
            is_async: resolved_async,
        };
    };

    // Check for Responds<typeof S> or Produces<typeof S, typeof C>.
    let TSType::TSTypeReference(tref) = inner else {
        return ReturnKind::Void {
            is_async: resolved_async,
        };
    };
    let void_kind = ReturnKind::Void {
        is_async: resolved_async,
    };
    match ts_type_name_local(&tref.type_name) {
        "Responds" => extract_typeof_at(tref, 0).map_or(void_kind, |schema| ReturnKind::Responds {
            schema,
            codec: None,
            is_async: resolved_async,
        }),
        "Produces" => {
            let schema = extract_typeof_at(tref, 0);
            let codec = extract_typeof_at(tref, 1);
            match (schema, codec) {
                (Some(schema), Some(codec)) => ReturnKind::Responds {
                    schema,
                    codec: Some(resolve_codec_ref(codec, codec_literals)),
                    is_async: resolved_async,
                },
                _ => void_kind,
            }
        }
        _ => void_kind,
    }
}

/// Extract the identifier from a `typeof X` type-query at position `index` in
/// `Wrapper<..., typeof X, ...>` (e.g. the `S` in `Body<typeof CreateUserBody>`,
/// or the `S`/`C` in `Produces<typeof S, typeof C>`).
fn extract_typeof_at(tref: &oxc_ast::ast::TSTypeReference<'_>, index: usize) -> Option<SchemaRef> {
    let params = tref.type_arguments.as_ref()?;
    let arg = params.params.get(index)?;
    let TSType::TSTypeQuery(query) = arg else {
        return None;
    };
    let ident = match &query.expr_name {
        TSTypeQueryExprName::IdentifierReference(id) => id.name.as_str(),
        _ => return None,
    };
    Some(SchemaRef {
        ident: ident.to_owned(),
    })
}

/// Return the rightmost (local) name from a `TSTypeName`.
/// `express.Request` → `"Request"`, `Request` → `"Request"`.
fn ts_type_name_local<'a>(name: &'a TSTypeName<'a>) -> &'a str {
    match name {
        TSTypeName::IdentifierReference(id) => id.name.as_str(),
        TSTypeName::QualifiedName(q) => q.right.name.as_str(),
        TSTypeName::ThisExpression(_) => "",
    }
}

/// Extract the string value from a call argument, or emit a diagnostic and return `None`.
fn extract_string_arg<'a>(
    arg: &'a Argument<'a>,
    decorator: &str,
    file_path: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Option<String> {
    match arg {
        Argument::StringLiteral(lit) => Some(lit.value.to_string()),
        Argument::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref().map(ToString::to_string)),
        other => {
            diagnostics.push(ParseDiagnostic {
                file: file_path.to_owned(),
                decorator: decorator.to_owned(),
                found: describe_arg(other),
                hint: "use a string literal, or re-run with --tsc to resolve constants".to_owned(),
            });
            None
        }
    }
}

/// Human-readable description of a non-literal argument for diagnostic output.
fn describe_arg(arg: &Argument<'_>) -> String {
    match arg {
        Argument::Identifier(id) => format!("identifier `{}`", id.name),
        Argument::TemplateLiteral(_) => "template literal with expressions".to_owned(),
        Argument::BinaryExpression(_) => "binary expression".to_owned(),
        _ => "non-literal expression".to_owned(),
    }
}

/// Extract the bare identifier name from a decorator callee expression.
fn decorator_ident_name(callee: &Expression<'_>) -> Option<String> {
    match callee {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// Join a controller base path with a route-level path, normalizing slashes.
fn join_paths(base: &str, route: &str) -> String {
    let b = base.trim_end_matches('/');
    let r = route.trim_start_matches('/');
    if b.is_empty() {
        format!("/{r}")
    } else if r.is_empty() {
        b.to_owned()
    } else {
        format!("{b}/{r}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> (Option<ControllerFile>, Vec<ParseDiagnostic>) {
        parse_source(src, "test.ts", Path::new("test.ts"))
    }

    #[test]
    fn parses_basic_controller() {
        let (file, diags) = parse(
            r#"
import { Controller, Get } from "@anvil-di/anvil-bellows";

@Controller("/users")
export class UserController {
  @Get("/:id")
  byId() {}
}
"#,
        );
        assert!(diags.is_empty());
        let file = file.unwrap();
        assert_eq!(file.controllers.len(), 1);
        let ctrl = &file.controllers[0];
        assert_eq!(ctrl.class_name, "UserController");
        assert_eq!(ctrl.routes.len(), 1);
        assert_eq!(ctrl.routes[0].method, HttpMethod::Get);
        assert_eq!(ctrl.routes[0].path, "/users/:id");
        assert_eq!(ctrl.routes[0].handler_name, "byId");
    }

    #[test]
    fn multiple_methods() {
        let (file, diags) = parse(
            r#"
@Controller("/items")
export class ItemController {
  @Get("/")
  list() {}
  @Post("/")
  create() {}
  @Put("/:id")
  update() {}
  @Delete("/:id")
  remove() {}
  @Patch("/:id")
  patch() {}
}
"#,
        );
        assert!(diags.is_empty());
        let ctrl = &file.unwrap().controllers[0];
        assert_eq!(ctrl.routes.len(), 5);
        assert_eq!(ctrl.routes[0].method, HttpMethod::Get);
        assert_eq!(ctrl.routes[1].method, HttpMethod::Post);
        assert_eq!(ctrl.routes[2].method, HttpMethod::Put);
        assert_eq!(ctrl.routes[3].method, HttpMethod::Delete);
        assert_eq!(ctrl.routes[4].method, HttpMethod::Patch);
    }

    #[test]
    fn non_literal_controller_arg_emits_diagnostic_and_skips() {
        let (file, diags) = parse(
            r#"
const BASE = "/users";
@Controller(BASE)
export class UserController {
  @Get("/:id")
  byId() {}
}
"#,
        );
        // The file has a controller but its path was non-literal — skipped entirely.
        assert!(file.is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "Controller");
        assert!(diags[0].found.contains("BASE"));
    }

    #[test]
    fn non_literal_route_arg_emits_diagnostic_skips_route() {
        let (file, diags) = parse(
            r#"
@Controller("/users")
export class UserController {
  @Get("/:id")
  byId() {}
  @Get(DYNAMIC_PATH)
  dynamic() {}
}
"#,
        );
        // One good route, one skipped.
        assert_eq!(diags.len(), 1);
        let ctrl = &file.unwrap().controllers[0];
        assert_eq!(ctrl.routes.len(), 1);
        assert_eq!(ctrl.routes[0].handler_name, "byId");
    }

    #[test]
    fn skips_non_controller_classes() {
        let (file, _) = parse(
            r"
export class NotAController {
  doThing() {}
}
",
        );
        assert!(file.is_none());
    }

    #[test]
    fn join_paths_combines_correctly() {
        assert_eq!(join_paths("/users", "/:id"), "/users/:id");
        assert_eq!(join_paths("/users/", "/:id"), "/users/:id");
        assert_eq!(join_paths("/users", "/"), "/users");
        assert_eq!(join_paths("", "/health"), "/health");
        assert_eq!(join_paths("/api/v1", "/users"), "/api/v1/users");
    }

    #[test]
    fn classify_body_query_params() {
        let (file, diags) = parse(
            r#"
import { Controller, Post } from "@anvil-di/anvil-bellows";
import type { Body, Query, Params } from "@anvil-di/anvil-bellows";

export const CreateBody = {};
export const FilterQuery = {};
export const UserParams = {};

@Controller("/users")
export class UserController {
  @Post("/:id")
  create(body: Body<typeof CreateBody>, query: Query<typeof FilterQuery>, params: Params<typeof UserParams>): void {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(route.params.len(), 3);
        assert_eq!(route.params[0].name, "body");
        assert_eq!(
            route.params[0].kind,
            ParamKind::Body(SchemaRef {
                ident: "CreateBody".into()
            })
        );
        assert_eq!(route.params[1].name, "query");
        assert_eq!(
            route.params[1].kind,
            ParamKind::Query(SchemaRef {
                ident: "FilterQuery".into()
            })
        );
        assert_eq!(route.params[2].name, "params");
        assert_eq!(
            route.params[2].kind,
            ParamKind::Params(SchemaRef {
                ident: "UserParams".into()
            })
        );
    }

    #[test]
    fn classify_request_response() {
        let (file, diags) = parse(
            r#"
import { Controller, Get } from "@anvil-di/anvil-bellows";

@Controller("/")
export class PingController {
  @Get("/ping")
  ping(req: Request, res: Response): void {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(route.params[0].kind, ParamKind::Request);
        assert_eq!(route.params[1].kind, ParamKind::Response);
    }

    #[test]
    fn classify_form_body_headers_raw_body() {
        let (file, diags) = parse(
            r#"
import { Controller, Post } from "@anvil-di/anvil-bellows";
import type { FormBody, Headers, RawBody } from "@anvil-di/anvil-bellows";

export const GatherBody = {};
export const SignatureHeaders = {};

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  gather(body: FormBody<typeof GatherBody>, headers: Headers<typeof SignatureHeaders>, raw: RawBody): void {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(route.params.len(), 3);
        assert_eq!(route.params[0].name, "body");
        assert_eq!(
            route.params[0].kind,
            ParamKind::FormBody(SchemaRef {
                ident: "GatherBody".into()
            })
        );
        assert_eq!(route.params[1].name, "headers");
        assert_eq!(
            route.params[1].kind,
            ParamKind::Headers(SchemaRef {
                ident: "SignatureHeaders".into()
            })
        );
        assert_eq!(route.params[2].name, "raw");
        assert_eq!(route.params[2].kind, ParamKind::RawBody);
    }

    #[test]
    fn classify_two_arg_body_resolves_codec_content_type() {
        let (file, diags) = parse(
            r#"
import { Controller, Post } from "@anvil-di/anvil-bellows";

export const GatherCallbackSchema = {};
export const twimlRequestCodec = { contentType: "application/xml", decode: (raw: Buffer) => ({}) };

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  gather(body: Body<typeof GatherCallbackSchema, typeof twimlRequestCodec>): void {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(route.params.len(), 1);
        assert_eq!(
            route.params[0].kind,
            ParamKind::Consumes {
                schema: SchemaRef {
                    ident: "GatherCallbackSchema".into()
                },
                codec: CodecRef {
                    ident: "twimlRequestCodec".into(),
                    content_type: Some("application/xml".into())
                }
            }
        );
    }

    #[test]
    fn classify_two_arg_body_with_unresolvable_codec_leaves_content_type_none() {
        let (file, diags) = parse(
            r#"
import { Controller, Post } from "@anvil-di/anvil-bellows";
import { importedCodec } from "./codecs";

export const GatherCallbackSchema = {};

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  gather(body: Body<typeof GatherCallbackSchema, typeof importedCodec>): void {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(
            route.params[0].kind,
            ParamKind::Consumes {
                schema: SchemaRef {
                    ident: "GatherCallbackSchema".into()
                },
                codec: CodecRef {
                    ident: "importedCodec".into(),
                    content_type: None
                }
            }
        );
    }

    #[test]
    fn sse_route_injects_stream_and_signal_and_registers_as_get() {
        let (file, diags) = parse(
            r#"
import { Controller, Sse } from "@anvil-di/anvil-bellows";

@Controller("/events")
export class EventsController {
  @Sse("/progress")
  async progress(stream: SseStream, signal: AbortSignal): Promise<void> {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert!(route.is_sse);
        assert_eq!(route.method, HttpMethod::Get);
        assert_eq!(route.params[0].kind, ParamKind::Sse);
        assert_eq!(route.params[1].kind, ParamKind::Signal);
    }

    #[test]
    fn sse_route_with_responds_return_type_rejected() {
        let (file, diags) = parse(
            r#"
import { Controller, Sse } from "@anvil-di/anvil-bellows";
export const EventSchema = {};

@Controller("/events")
export class EventsController {
  @Sse("/progress")
  progress(stream: SseStream): Responds<typeof EventSchema> { return {} as any; }
}
"#,
        );
        assert!(
            file.is_none(),
            "route with a Responds<S> return type must be dropped, leaving no controllers"
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "Sse");
    }

    #[test]
    fn classify_responds_return_type() {
        let (file, diags) = parse(
            r#"
import { Controller, Get } from "@anvil-di/anvil-bellows";
export const UserSchema = {};

@Controller("/users")
export class UserController {
  @Get("/:id")
  byId(req: Request): Responds<typeof UserSchema> { return {} as any; }
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(
            route.return_kind,
            ReturnKind::Responds {
                schema: SchemaRef {
                    ident: "UserSchema".into()
                },
                codec: None,
                is_async: false
            }
        );
    }

    #[test]
    fn classify_produces_return_type() {
        let (file, diags) = parse(
            r#"
import { Controller, Post } from "@anvil-di/anvil-bellows";
export const TwimlResponseSchema = {};
export const twimlCodec = { contentType: "application/xml", encode: (v: unknown) => "" };

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  gather(req: Request): Produces<typeof TwimlResponseSchema, typeof twimlCodec> { return {} as any; }
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(
            route.return_kind,
            ReturnKind::Responds {
                schema: SchemaRef {
                    ident: "TwimlResponseSchema".into()
                },
                codec: Some(CodecRef {
                    ident: "twimlCodec".into(),
                    content_type: Some("application/xml".into())
                }),
                is_async: false
            }
        );
    }

    #[test]
    fn classify_promise_produces_return_type() {
        let (file, diags) = parse(
            r#"
import { Controller, Post } from "@anvil-di/anvil-bellows";
export const TwimlResponseSchema = {};
export const twimlCodec = { contentType: "application/xml", encode: (v: unknown) => "" };

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  async gather(req: Request): Promise<Produces<typeof TwimlResponseSchema, typeof twimlCodec>> { return {} as any; }
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(
            route.return_kind,
            ReturnKind::Responds {
                schema: SchemaRef {
                    ident: "TwimlResponseSchema".into()
                },
                codec: Some(CodecRef {
                    ident: "twimlCodec".into(),
                    content_type: Some("application/xml".into())
                }),
                is_async: true
            }
        );
    }

    #[test]
    fn classify_promise_responds_return_type() {
        let (file, diags) = parse(
            r#"
import { Controller, Get } from "@anvil-di/anvil-bellows";
export const UserSchema = {};

@Controller("/users")
export class UserController {
  @Get("/:id")
  async byId(req: Request): Promise<Responds<typeof UserSchema>> { return {} as any; }
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(
            route.return_kind,
            ReturnKind::Responds {
                schema: SchemaRef {
                    ident: "UserSchema".into()
                },
                codec: None,
                is_async: true
            }
        );
    }

    #[test]
    fn class_and_method_middleware_ordered_and_combined() {
        let (file, diags) = parse(
            r#"
import { Controller, Get, Middleware } from "@anvil-di/anvil-bellows";
import { requireAuth } from "./auth";

@Controller("/admin")
@Middleware(requireAuth)
export class AdminController {
  @Get("/stats")
  @Middleware(requireAdmin)
  stats() {}

  @Get("/ping")
  ping() {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ctrl = &file.unwrap().controllers[0];

        // Class-level middleware runs first, then method-level, in source order.
        let stats = &ctrl.routes[0];
        assert_eq!(
            stats
                .middleware
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec!["requireAuth", "requireAdmin"]
        );
        assert!(matches!(
            stats.middleware[0].origin,
            Some(ImportOrigin::Relative(_))
        ));
        // `requireAdmin` isn't imported anywhere in this file.
        assert_eq!(stats.middleware[1].origin, None);

        // Class-level middleware applies even to routes with no method-level `@Middleware`.
        let ping = &ctrl.routes[1];
        assert_eq!(
            ping.middleware
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec!["requireAuth"]
        );
    }

    #[test]
    fn middleware_resolves_package_import() {
        let (file, diags) = parse(
            r#"
import { Controller, Get, Middleware } from "@anvil-di/anvil-bellows";
import rateLimit from "express-rate-limit";

@Controller("/api")
export class ApiController {
  @Get("/")
  @Middleware(rateLimit)
  list() {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let route = &file.unwrap().controllers[0].routes[0];
        assert_eq!(
            route.middleware[0].origin,
            Some(ImportOrigin::Package("express-rate-limit".into()))
        );
    }

    #[test]
    fn non_identifier_middleware_arg_emits_diagnostic() {
        let (file, diags) = parse(
            r#"
import { Controller, Get, Middleware } from "@anvil-di/anvil-bellows";

@Controller("/api")
export class ApiController {
  @Get("/")
  @Middleware("not-an-identifier")
  list() {}
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "Middleware");
        let route = &file.unwrap().controllers[0].routes[0];
        assert!(route.middleware.is_empty());
    }

    #[test]
    fn class_and_method_auth_ordered_and_combined() {
        let (file, diags) = parse(
            r#"
import { Controller, Get, Authn, Authz } from "@anvil-di/anvil-bellows";
import { SessionAuthn } from "./session-authn";
import { RoleAuthz } from "./role-authz";

@Controller("/admin")
@Authn(SessionAuthn)
export class AdminController {
  @Get("/stats")
  @Authz(RoleAuthz)
  stats() {}

  @Get("/ping")
  ping() {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ctrl = &file.unwrap().controllers[0];

        // Class-level @Authn applies to every route; method-level @Authz only to `stats`.
        let stats = &ctrl.routes[0];
        assert_eq!(
            stats
                .authn
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["SessionAuthn"]
        );
        assert_eq!(
            stats
                .authz
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["RoleAuthz"]
        );

        let ping = &ctrl.routes[1];
        assert_eq!(
            ping.authn
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["SessionAuthn"]
        );
        assert!(ping.authz.is_empty());
    }

    #[test]
    fn non_identifier_auth_arg_emits_diagnostic() {
        let (file, diags) = parse(
            r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";

@Controller("/api")
export class ApiController {
  @Get("/")
  @Authn("not-an-identifier")
  list() {}
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "Authn");
        let route = &file.unwrap().controllers[0].routes[0];
        assert!(route.authn.is_empty());
    }

    #[test]
    fn authn_scheme_extracted_from_direct_implements_literal() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("session-authn.ts"),
            r#"
import type { AuthnService, AuthnResult } from "@anvil-di/bellows";
export class SessionAuthn implements AuthnService<{ id: string }, "bearerAuth"> {
  identify(req: unknown): AuthnResult<{ id: string }> { return { identified: false }; }
}
"#,
        )
        .unwrap();

        let controller_path = dir.path().join("admin-controller.ts");
        let src = r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";
import { SessionAuthn } from "./session-authn";

@Controller("/admin")
@Authn(SessionAuthn)
export class AdminController {
  @Get("/stats")
  stats() {}
}
"#;
        std::fs::write(&controller_path, src).unwrap();

        let (file, diags) = parse_source(
            src,
            &controller_path.display().to_string(),
            &controller_path,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ctrl = &file.unwrap().controllers[0];
        assert_eq!(
            ctrl.routes[0].authn[0].scheme.as_deref(),
            Some("bearerAuth")
        );
    }

    #[test]
    fn authn_scheme_omitted_when_implements_clause_missing() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("session-authn.ts"),
            r"
export class SessionAuthn {
  identify(req: unknown) { return { identified: false }; }
}
",
        )
        .unwrap();

        let controller_path = dir.path().join("admin-controller.ts");
        let src = r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";
import { SessionAuthn } from "./session-authn";

@Controller("/admin")
@Authn(SessionAuthn)
export class AdminController {
  @Get("/stats")
  stats() {}
}
"#;
        std::fs::write(&controller_path, src).unwrap();

        let (file, diags) = parse_source(
            src,
            &controller_path.display().to_string(),
            &controller_path,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ctrl = &file.unwrap().controllers[0];
        assert_eq!(ctrl.routes[0].authn[0].scheme, None);
    }

    #[test]
    fn authn_scheme_omitted_for_package_origin() {
        let (file, diags) = parse(
            r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";
import { SomeAuthn } from "some-auth-package";

@Controller("/admin")
@Authn(SomeAuthn)
export class AdminController {
  @Get("/stats")
  stats() {}
}
"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ctrl = &file.unwrap().controllers[0];
        assert_eq!(ctrl.routes[0].authn[0].scheme, None);
    }

    #[test]
    fn authn_user_valid_when_all_services_agree() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("user-types.ts"),
            r"
export interface AdminUser {
  id: string;
}
",
        )
        .unwrap();

        std::fs::write(
            dir.path().join("session-authn.ts"),
            r#"
import type { AuthnService, AuthnResult } from "@anvil-di/bellows";
import type { AdminUser } from "./user-types";
export class SessionAuthn implements AuthnService<AdminUser, "bearerAuth"> {
  identify(req: unknown): AuthnResult<AdminUser> { return { identified: false }; }
}
"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("api-key-authn.ts"),
            r#"
import type { AuthnService, AuthnResult } from "@anvil-di/bellows";
import type { AdminUser } from "./user-types";
export class ApiKeyAuthn implements AuthnService<AdminUser, "apiKeyAuth"> {
  identify(req: unknown): AuthnResult<AdminUser> { return { identified: false }; }
}
"#,
        )
        .unwrap();

        let controller_path = dir.path().join("admin-controller.ts");
        let src = r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";
import type { AuthnUser } from "@anvil-di/bellows";
import type { AdminUser } from "./user-types";
import { SessionAuthn } from "./session-authn";
import { ApiKeyAuthn } from "./api-key-authn";

@Controller("/admin")
@Authn(SessionAuthn, ApiKeyAuthn)
export class AdminController {
  @Get("/stats")
  stats(user: AuthnUser<AdminUser>): void {}
}
"#;
        std::fs::write(&controller_path, src).unwrap();

        let (file, diags) = parse_source(
            src,
            &controller_path.display().to_string(),
            &controller_path,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ctrl = &file.unwrap().controllers[0];
        assert_eq!(ctrl.routes.len(), 1);
        let ParamKind::User(user_ref) = &ctrl.routes[0].params[0].kind else {
            panic!("expected ParamKind::User");
        };
        assert_eq!(user_ref.type_name, "AdminUser");
        assert_eq!(
            user_ref.identity.as_ref().unwrap().name,
            "AdminUser".to_owned()
        );
    }

    #[test]
    fn authn_user_rejected_when_services_disagree() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("admin-authn.ts"),
            r#"
import type { AuthnService, AuthnResult } from "@anvil-di/bellows";
export interface AdminUser { id: string; }
export class AdminAuthn implements AuthnService<AdminUser, "bearerAuth"> {
  identify(req: unknown): AuthnResult<AdminUser> { return { identified: false }; }
}
"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("guest-authn.ts"),
            r#"
import type { AuthnService, AuthnResult } from "@anvil-di/bellows";
export interface GuestUser { sessionId: string; }
export class GuestAuthn implements AuthnService<GuestUser, "cookieAuth"> {
  identify(req: unknown): AuthnResult<GuestUser> { return { identified: false }; }
}
"#,
        )
        .unwrap();

        let controller_path = dir.path().join("admin-controller.ts");
        let src = r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";
import type { AuthnUser } from "@anvil-di/bellows";
import type { AdminUser } from "./admin-authn";
import { AdminAuthn } from "./admin-authn";
import { GuestAuthn } from "./guest-authn";

@Controller("/admin")
@Authn(AdminAuthn, GuestAuthn)
export class AdminController {
  @Get("/stats")
  stats(user: AuthnUser<AdminUser>): void {}
}
"#;
        std::fs::write(&controller_path, src).unwrap();

        let (file, diags) = parse_source(
            src,
            &controller_path.display().to_string(),
            &controller_path,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "AuthnUser");
        assert!(diags[0].found.contains("different user types"));
        // The route is dropped entirely — no controller survives with 0 routes.
        assert!(file.is_none());
    }

    #[test]
    fn authn_user_rejected_when_no_authn_declared() {
        let (file, diags) = parse(
            r#"
import { Controller, Get } from "@anvil-di/anvil-bellows";
import type { AuthnUser } from "@anvil-di/bellows";

interface AdminUser { id: string; }

@Controller("/admin")
export class AdminController {
  @Get("/stats")
  stats(user: AuthnUser<AdminUser>): void {}
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "AuthnUser");
        assert!(diags[0].found.contains("no @Authn services declared"));
        assert!(file.is_none());
    }

    #[test]
    fn authn_user_rejected_when_service_unresolvable() {
        let (file, diags) = parse(
            r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";
import type { AuthnUser } from "@anvil-di/bellows";
import { SomeAuthn } from "some-auth-package";

interface AdminUser { id: string; }

@Controller("/admin")
@Authn(SomeAuthn)
export class AdminController {
  @Get("/stats")
  stats(user: AuthnUser<AdminUser>): void {}
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "AuthnUser");
        assert!(diags[0].found.contains("no directly-resolvable user type"));
        assert!(file.is_none());
    }

    #[test]
    fn authn_user_rejected_when_t_is_inline_type() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("session-authn.ts"),
            r#"
import type { AuthnService, AuthnResult } from "@anvil-di/bellows";
export interface AdminUser { id: string; }
export class SessionAuthn implements AuthnService<AdminUser, "bearerAuth"> {
  identify(req: unknown): AuthnResult<AdminUser> { return { identified: false }; }
}
"#,
        )
        .unwrap();

        let controller_path = dir.path().join("admin-controller.ts");
        let src = r#"
import { Controller, Get, Authn } from "@anvil-di/anvil-bellows";
import type { AuthnUser } from "@anvil-di/bellows";
import { SessionAuthn } from "./session-authn";

@Controller("/admin")
@Authn(SessionAuthn)
export class AdminController {
  @Get("/stats")
  stats(user: AuthnUser<{ id: string }>): void {}
}
"#;
        std::fs::write(&controller_path, src).unwrap();

        let (file, diags) = parse_source(
            src,
            &controller_path.display().to_string(),
            &controller_path,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].decorator, "AuthnUser");
        assert!(diags[0]
            .found
            .contains("not a directly-resolvable named type"));
        assert!(file.is_none());
    }
}
