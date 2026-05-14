//! Parse TypeScript controller files for `@Controller`, `@Get`, `@Post`, etc.
//!
//! Static mode only — accepts string literal decorator arguments. Non-literal
//! arguments emit a [`ParseDiagnostic`] and skip the affected route.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, ClassElement, Declaration, Decorator, Expression, FormalParameter,
    ImportDeclarationSpecifier, MethodDefinitionKind, PropertyKey, Statement, TSType, TSTypeName,
    TSTypeQueryExprName,
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

/// Classification of a handler parameter's type annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKind {
    /// `Body<typeof S>` — validate `req.body` against schema `S`.
    Body(SchemaRef),
    /// `Query<typeof S>` — validate `req.query` against schema `S`.
    Query(SchemaRef),
    /// `Params<typeof S>` — validate `req.params` against schema `S`.
    Params(SchemaRef),
    /// `Request` or `express.Request` — inject the raw request object.
    Request,
    /// `Response` or `express.Response` — inject the raw response object.
    Response,
    /// Unrecognized annotation — triggers v0.1 passthrough for the whole route.
    Unknown,
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
    /// `Responds<typeof S>` or `Promise<Responds<typeof S>>`.
    Responds {
        /// Schema used to validate the return value before calling `res.json()`.
        schema: SchemaRef,
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
    /// Typed parameters of the handler (empty means v0.1 passthrough).
    pub params: Vec<TypedParam>,
    /// Classification of the handler's return type.
    pub return_kind: ReturnKind,
    /// True when `@Deprecated` is present on the method.
    pub deprecated: bool,
    /// Additional responses from `@Returns(status, schema)` decorators.
    pub extra_responses: Vec<ExtraResponse>,
}

/// A single constructor parameter on a `@Controller` class.
#[derive(Debug, Clone)]
pub struct CtorParam {
    /// The parameter's local name (e.g. `todoService`).
    pub name: String,
    /// The TypeScript type name (e.g. `TodoService`).
    pub type_name: String,
    /// Absolute path to the file that exports `type_name`, if it could be
    /// resolved from a relative import in the controller file. `None` when the
    /// dep comes from a package or when the import is unresolvable.
    pub abs_source: Option<PathBuf>,
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
#[allow(clippy::too_many_lines)]
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

        let (tags, security) = extract_class_metadata(&class.decorators);
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

            let typed_params: Vec<TypedParam> =
                m.value.params.items.iter().map(classify_param).collect();

            let return_kind = classify_return(
                m.value.return_type.as_ref().map(|a| &a.type_annotation),
                m.value.r#async,
            );

            // Scan all method decorators: find HTTP method + collect modifiers.
            let mut route_opt: Option<Route> = None;
            let mut deprecated = false;
            let mut extra_responses: Vec<ExtraResponse> = Vec::new();

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
                }
            }

            if let Some(mut route) = route_opt {
                route.deprecated = deprecated;
                route.extra_responses = extra_responses;
                routes.push(route);
            }
        }

        if !routes.is_empty() {
            controllers.push(Controller {
                class_name,
                ctor_params,
                routes,
                tags,
                security,
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

/// Walk import declarations in `stmts` and build a map from local name to the
/// absolute path of the source module (for relative specifiers only).
fn collect_import_map(stmts: &[Statement<'_>], file_dir: &Path) -> HashMap<String, PathBuf> {
    let mut map: HashMap<String, PathBuf> = HashMap::new();
    for stmt in stmts {
        let Statement::ImportDeclaration(import) = stmt else {
            continue;
        };
        let raw_spec = import.source.value.as_str();
        // Only resolve relative imports; skip package names.
        if !raw_spec.starts_with("./") && !raw_spec.starts_with("../") {
            continue;
        }
        // Strip .js / .ts extension for path resolution then add .ts.
        let stem = raw_spec
            .strip_suffix(".js")
            .or_else(|| raw_spec.strip_suffix(".ts"))
            .unwrap_or(raw_spec);
        let abs = file_dir.join(format!("{stem}.ts"));
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
            map.insert(local_name, abs.clone());
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
    import_map: &HashMap<String, PathBuf>,
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
        "Body" | "Query" | "Params" | "Responds" | "Request" | "Response" => return None,
        _ => {}
    }
    let type_name = type_name.to_owned();
    let abs_source = import_map.get(&type_name).cloned();
    Some(CtorParam {
        name,
        type_name,
        abs_source,
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
    let method = HttpMethod::from_decorator_name(&name)?;
    let arg = call.arguments.first()?;
    let route_path = extract_string_arg(arg, &name, file_path, diagnostics)?;
    let full_path = join_paths(base_path, &route_path);
    Some(Route {
        method,
        path: full_path,
        handler_name: handler_name.to_owned(),
        params: typed_params.to_vec(),
        return_kind,
        deprecated: false, // filled in by caller after scanning all decorators
        extra_responses: vec![], // ditto
    })
}

/// Extract `@Tag("name")` and `@Security("scheme")` from class decorators.
fn extract_class_metadata(decorators: &[Decorator<'_>]) -> (Vec<String>, Vec<String>) {
    let mut tags = Vec::new();
    let mut security = Vec::new();
    for dec in decorators {
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
    (tags, security)
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
fn classify_param(param: &FormalParameter<'_>) -> TypedParam {
    let name = match &param.pattern {
        BindingPattern::BindingIdentifier(id) => id.name.as_str().to_owned(),
        _ => "_".to_owned(),
    };
    let kind = param
        .type_annotation
        .as_ref()
        .map_or(ParamKind::Unknown, |ann| {
            classify_ts_type(&ann.type_annotation)
        });
    TypedParam { name, kind }
}

/// Classify a `TSType` into a `ParamKind`.
fn classify_ts_type(ty: &TSType<'_>) -> ParamKind {
    let TSType::TSTypeReference(tref) = ty else {
        return ParamKind::Unknown;
    };
    let local_name = ts_type_name_local(&tref.type_name);
    match local_name {
        "Request" => return ParamKind::Request,
        "Response" => return ParamKind::Response,
        "Body" | "Query" | "Params" => {}
        _ => return ParamKind::Unknown,
    }
    let Some(schema) = extract_typeof_schema_from_tref(tref) else {
        return ParamKind::Unknown;
    };
    match local_name {
        "Body" => ParamKind::Body(schema),
        "Query" => ParamKind::Query(schema),
        "Params" => ParamKind::Params(schema),
        _ => unreachable!(),
    }
}

/// Classify the return type annotation into a `ReturnKind`.
fn classify_return(ty: Option<&TSType<'_>>, is_async: bool) -> ReturnKind {
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

    // Check for Responds<typeof S>.
    let TSType::TSTypeReference(tref) = inner else {
        return ReturnKind::Void {
            is_async: resolved_async,
        };
    };
    if ts_type_name_local(&tref.type_name) != "Responds" {
        return ReturnKind::Void {
            is_async: resolved_async,
        };
    }
    if let Some(schema) = extract_typeof_schema_from_tref(tref) {
        ReturnKind::Responds {
            schema,
            is_async: resolved_async,
        }
    } else {
        ReturnKind::Void {
            is_async: resolved_async,
        }
    }
}

/// Extract the schema identifier from `Wrapper<typeof S>` (e.g. `Body<typeof CreateUserBody>`).
fn extract_typeof_schema_from_tref(tref: &oxc_ast::ast::TSTypeReference<'_>) -> Option<SchemaRef> {
    let params = tref.type_arguments.as_ref()?;
    let first = params.params.first()?;
    let TSType::TSTypeQuery(query) = first else {
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
                is_async: false
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
                is_async: true
            }
        );
    }
}
