//! Parse TypeScript controller files for `@Controller`, `@Get`, `@Post`, etc.
//!
//! Static mode only — accepts string literal decorator arguments. Non-literal
//! arguments emit a [`ParseDiagnostic`] and skip the affected route.

use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ClassElement, Declaration, Decorator, Expression, MethodDefinitionKind, PropertyKey,
    Statement,
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

/// A single route extracted from a method decorator.
#[derive(Debug, Clone)]
pub struct Route {
    /// HTTP method.
    pub method: HttpMethod,
    /// The full path: `base_path` joined with the route decorator path.
    pub path: String,
    /// The TypeScript method name on the controller class.
    pub handler_name: String,
}

/// A controller class extracted from a source file.
#[derive(Debug, Clone)]
pub struct Controller {
    /// TypeScript class name.
    pub class_name: String,
    /// Routes declared on this controller.
    pub routes: Vec<Route>,
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
        if name.ends_with(".d.ts")
            || name.ends_with(".test.ts")
            || name == "routes.module.ts"
        {
            continue;
        }

        let source = std::fs::read_to_string(&path)?;
        let (file_opt, mut diags) =
            parse_source(&source, &path.display().to_string(), &path);
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

        let mut routes: Vec<Route> = Vec::new();

        for element in &class.body.body {
            let ClassElement::MethodDefinition(m) = element else {
                continue;
            };
            if m.kind == MethodDefinitionKind::Constructor {
                continue;
            }

            let handler_name = match &m.key {
                PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                PropertyKey::StringLiteral(s) => s.value.to_string(),
                _ => continue,
            };

            for dec in &m.decorators {
                if let Some(route) = try_extract_route(
                    dec,
                    &base_path,
                    &handler_name,
                    file_path,
                    &mut diagnostics,
                ) {
                    routes.push(route);
                    break;
                }
            }
        }

        if !routes.is_empty() {
            controllers.push(Controller { class_name, routes });
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
    })
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
                hint: "use a string literal, or re-run with --tsc to resolve constants"
                    .to_owned(),
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
import { Controller, Get } from "@msulak/anvil-bellows";

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
            r#"
export class NotAController {
  doThing() {}
}
"#,
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
}
