//! Expand `bellowsCompose(...)` composite decorators (ADR-0008).
//!
//! A bare decorator `@X` on a controller or route is looked up through the
//! use-site file's imports, re-exports, and packages. When it is a composite,
//! its contents expand in place into the same items a hand-written decorator
//! stack produces; anything else is an ordinary decorator and is ignored.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, Declaration, Expression, ImportDeclarationSpecifier, Statement,
    TSImportTypeQualifier, TSType, VariableDeclaration, VariableDeclarationKind,
};
use oxc_parser::Parser;
use oxc_resolver::{ResolveOptions, Resolver, TsconfigDiscovery};
use oxc_span::SourceType;

use crate::parser::{
    classify_decorator, collect_import_map, decorator_ident_name, DecoratorItem, ImportOrigin,
    ParseDiagnostic,
};

/// Bounds re-export chains, which can loop through barrels.
const MAX_EXPORT_DEPTH: usize = 32;

/// Where a decorator is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Site {
    Class,
    Method,
}

impl Site {
    fn allows(self, item: &DecoratorItem) -> bool {
        match item {
            DecoratorItem::Middleware(_) | DecoratorItem::Authn(_) | DecoratorItem::Authz(_) => {
                true
            }
            DecoratorItem::Tag(_) | DecoratorItem::Security(_) => self == Self::Class,
            DecoratorItem::Returns(_) => self == Self::Method,
            DecoratorItem::Deprecated => false,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Class => "a controller class",
            Self::Method => "a route method",
        }
    }
}

/// One argument of a `bellowsCompose(...)` call.
#[derive(Debug, Clone)]
enum Part {
    Item(DecoratorItem),
    /// Another composite, named as it is bound in the definition file.
    Nested(String),
}

#[derive(Debug, Clone)]
enum ConstKind {
    Composite {
        parts: Vec<Part>,
        diagnostics: Vec<ParseDiagnostic>,
    },
    /// Typed `BellowsComposite` but without a `bellowsCompose(...)` initializer.
    TypedOnly,
    Other,
}

#[derive(Debug, Clone)]
enum ExportTarget {
    Local(String),
    From { spec: String, name: String },
}

#[derive(Debug, Default)]
struct FileIndex {
    consts: HashMap<String, ConstKind>,
    /// Local name → (specifier, imported name). Default and namespace imports
    /// are absent: a composite is always a named export.
    imports: HashMap<String, (String, String)>,
    exports: HashMap<String, ExportTarget>,
    star_exports: Vec<String>,
}

struct Definition {
    file: PathBuf,
    name: String,
    kind: ConstKind,
}

/// Resolves and expands composites, caching each file it reads.
pub(crate) struct Composites {
    resolver: Resolver,
    files: HashMap<PathBuf, Option<FileIndex>>,
    reported: HashSet<(PathBuf, String)>,
}

impl Composites {
    pub(crate) fn new() -> Self {
        let strings = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let options = ResolveOptions {
            extensions: strings(&[".ts", ".tsx", ".d.ts", ".js", ".jsx"]),
            extension_alias: vec![(".js".to_owned(), strings(&[".ts", ".d.ts", ".js"]))],
            condition_names: strings(&["types", "import", "node"]),
            main_fields: strings(&["types", "typings", "module", "main"]),
            tsconfig: Some(TsconfigDiscovery::Auto),
            ..ResolveOptions::default()
        };
        Self {
            resolver: Resolver::new(options),
            files: HashMap::new(),
            reported: HashSet::new(),
        }
    }

    /// Expand `@name` used in `use_file` (whose text is `use_source`).
    /// `None` when `name` isn't a composite. Diagnostics go to `diagnostics`;
    /// items not allowed at `site` are dropped with a diagnostic.
    pub(crate) fn expand(
        &mut self,
        use_file: &Path,
        use_source: &str,
        name: &str,
        site: Site,
        display_file: &str,
        diagnostics: &mut Vec<ParseDiagnostic>,
    ) -> Option<Vec<DecoratorItem>> {
        if !self.files.contains_key(use_file) {
            let index = index_source(use_file, use_source);
            self.files.insert(use_file.to_path_buf(), index);
        }
        let def = self.lookup_local(use_file, name, 0)?;
        let (parts, def_diagnostics) = match def.kind {
            ConstKind::Other => return None,
            ConstKind::TypedOnly => {
                diagnostics.push(typed_only_diagnostic(name, &def.file, display_file));
                return Some(Vec::new());
            }
            ConstKind::Composite {
                parts,
                diagnostics: def_diagnostics,
            } => (parts, def_diagnostics),
        };

        let key = (def.file.clone(), def.name.clone());
        let report = self.reported.insert(key.clone());
        if report {
            diagnostics.extend(def_diagnostics);
        }
        let mut items = Vec::new();
        let mut stack = vec![key];
        self.flatten(
            use_file,
            &def.file,
            parts,
            &mut stack,
            &mut items,
            diagnostics,
            report,
        );

        let (allowed, rejected): (Vec<_>, Vec<_>) =
            items.into_iter().partition(|item| site.allows(item));
        for item in rejected {
            diagnostics.push(ParseDiagnostic {
                file: display_file.to_owned(),
                decorator: name.to_owned(),
                summary: "contains a decorator that doesn't apply here".to_owned(),
                found: format!("@{} on {}", item.decorator_name(), site.label()),
                hint: "@Tag and @Security apply to controller classes; @Returns applies to \
                       route methods"
                    .to_owned(),
            });
        }
        Some(allowed)
    }

    /// `report` is true the first time this definition is expanded, so its
    /// diagnostics are emitted once however many routes use it.
    #[allow(clippy::too_many_arguments)]
    fn flatten(
        &mut self,
        use_file: &Path,
        def_file: &Path,
        parts: Vec<Part>,
        stack: &mut Vec<(PathBuf, String)>,
        out: &mut Vec<DecoratorItem>,
        diagnostics: &mut Vec<ParseDiagnostic>,
        report: bool,
    ) {
        for part in parts {
            match part {
                Part::Item(item) => out.push(rebase_origins(item, def_file, use_file)),
                Part::Nested(nested) => {
                    let display = def_file.display().to_string();
                    let Some(def) = self.lookup_local(def_file, &nested, 0) else {
                        if report {
                            diagnostics.push(not_a_composite_diagnostic(&nested, &display));
                        }
                        continue;
                    };
                    let key = (def.file.clone(), def.name.clone());
                    match def.kind {
                        ConstKind::Other if report => {
                            diagnostics.push(not_a_composite_diagnostic(&nested, &display));
                        }
                        ConstKind::TypedOnly if report => {
                            diagnostics.push(typed_only_diagnostic(&nested, &def.file, &display));
                        }
                        ConstKind::Other | ConstKind::TypedOnly => {}
                        ConstKind::Composite { .. } if stack.contains(&key) => {
                            if !report {
                                continue;
                            }
                            diagnostics.push(ParseDiagnostic {
                                file: display,
                                decorator: "bellowsCompose".to_owned(),
                                summary: "composites reference each other in a cycle".to_owned(),
                                found: stack
                                    .iter()
                                    .map(|(_, n)| n.as_str())
                                    .chain(std::iter::once(nested.as_str()))
                                    .collect::<Vec<_>>()
                                    .join(" → "),
                                hint: "remove one of the nested composites".to_owned(),
                            });
                        }
                        ConstKind::Composite {
                            parts: nested_parts,
                            diagnostics: nested_diagnostics,
                        } => {
                            let nested_report = self.reported.insert(key.clone());
                            if nested_report {
                                diagnostics.extend(nested_diagnostics);
                            }
                            stack.push(key);
                            self.flatten(
                                use_file,
                                &def.file,
                                nested_parts,
                                stack,
                                out,
                                diagnostics,
                                nested_report,
                            );
                            stack.pop();
                        }
                    }
                }
            }
        }
    }

    /// Find what `local` is bound to in `file`: a `const` declared there, or
    /// a named import followed to its export.
    fn lookup_local(&mut self, file: &Path, local: &str, depth: usize) -> Option<Definition> {
        let index = self.index(file)?;
        if let Some(kind) = index.consts.get(local) {
            return Some(Definition {
                file: file.to_path_buf(),
                name: local.to_owned(),
                kind: kind.clone(),
            });
        }
        let (spec, imported) = index.imports.get(local)?.clone();
        let target = self.resolve(file, &spec)?;
        self.lookup_export(&target, &imported, depth + 1)
    }

    /// Find the declaration behind the export `name` of `file`, following
    /// `export { … } from` and `export * from` re-exports.
    fn lookup_export(&mut self, file: &Path, name: &str, depth: usize) -> Option<Definition> {
        if depth > MAX_EXPORT_DEPTH {
            return None;
        }
        let index = self.index(file)?;
        match index.exports.get(name).cloned() {
            Some(ExportTarget::Local(local)) => self.lookup_local(file, &local, depth),
            Some(ExportTarget::From { spec, name }) => {
                let target = self.resolve(file, &spec)?;
                self.lookup_export(&target, &name, depth + 1)
            }
            None => {
                let stars = index.star_exports.clone();
                stars.iter().find_map(|spec| {
                    let target = self.resolve(file, spec)?;
                    self.lookup_export(&target, name, depth + 1)
                })
            }
        }
    }

    fn index(&mut self, file: &Path) -> Option<&FileIndex> {
        if !self.files.contains_key(file) {
            let index = std::fs::read_to_string(file)
                .ok()
                .and_then(|source| index_source(file, &source));
            self.files.insert(file.to_path_buf(), index);
        }
        self.files.get(file)?.as_ref()
    }

    fn resolve(&self, importer: &Path, spec: &str) -> Option<PathBuf> {
        // `resolve_file` panics on a path that isn't a file.
        if !importer.is_file() {
            return None;
        }
        // Canonicalized to match `parse_entry`'s paths: on Windows that is
        // the `\\?\` form, which `oxc_resolver` doesn't return.
        let path = self.resolver.resolve_file(importer, spec).ok()?.full_path();
        Some(std::fs::canonicalize(&path).unwrap_or(path))
    }
}

fn index_source(file: &Path, source: &str) -> Option<FileIndex> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(file).unwrap_or_else(|_| SourceType::ts());
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        return None;
    }
    let file_dir = file.parent().unwrap_or(Path::new("."));
    let import_map = collect_import_map(&ret.program.body, file_dir);
    let display = file.display().to_string();

    let mut index = FileIndex::default();
    for stmt in &ret.program.body {
        match stmt {
            Statement::ImportDeclaration(import) => {
                let spec = import.source.value.to_string();
                for s in import.specifiers.iter().flatten() {
                    if let ImportDeclarationSpecifier::ImportSpecifier(s) = s {
                        index.imports.insert(
                            s.local.name.to_string(),
                            (spec.clone(), s.imported.name().to_string()),
                        );
                    }
                }
            }
            Statement::VariableDeclaration(decl) => {
                index_consts(decl, &import_map, &display, &mut index, false);
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(Declaration::VariableDeclaration(decl)) = &export.declaration {
                    index_consts(decl, &import_map, &display, &mut index, true);
                }
                for s in &export.specifiers {
                    let exported = s.exported.name().to_string();
                    let local = s.local.name().to_string();
                    let target = match &export.source {
                        Some(src) => ExportTarget::From {
                            spec: src.value.to_string(),
                            name: local,
                        },
                        None => ExportTarget::Local(local),
                    };
                    index.exports.insert(exported, target);
                }
            }
            Statement::ExportAllDeclaration(export) if export.exported.is_none() => {
                index.star_exports.push(export.source.value.to_string());
            }
            _ => {}
        }
    }
    Some(index)
}

fn index_consts(
    decl: &VariableDeclaration<'_>,
    import_map: &HashMap<String, ImportOrigin>,
    display_file: &str,
    index: &mut FileIndex,
    exported: bool,
) {
    if decl.kind != VariableDeclarationKind::Const {
        return;
    }
    for d in &decl.declarations {
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            continue;
        };
        let name = id.name.to_string();
        let kind = match &d.init {
            Some(Expression::CallExpression(call))
                if decorator_ident_name(&call.callee).as_deref() == Some("bellowsCompose") =>
            {
                let mut diagnostics = Vec::new();
                let parts = call
                    .arguments
                    .iter()
                    .filter_map(|arg| {
                        classify_part(arg, import_map, display_file, &mut diagnostics)
                    })
                    .collect();
                ConstKind::Composite { parts, diagnostics }
            }
            _ if d
                .type_annotation
                .as_ref()
                .is_some_and(|a| is_composite_type(&a.type_annotation)) =>
            {
                ConstKind::TypedOnly
            }
            _ => ConstKind::Other,
        };
        if exported {
            index
                .exports
                .insert(name.clone(), ExportTarget::Local(name.clone()));
        }
        index.consts.insert(name, kind);
    }
}

fn classify_part(
    arg: &Argument<'_>,
    import_map: &HashMap<String, ImportOrigin>,
    display_file: &str,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Option<Part> {
    let unsupported = |found: String, hint: &str| ParseDiagnostic {
        file: display_file.to_owned(),
        decorator: "bellowsCompose".to_owned(),
        summary: "argument is not supported".to_owned(),
        found,
        hint: hint.to_owned(),
    };
    if let Argument::Identifier(id) = arg {
        return Some(Part::Nested(id.name.to_string()));
    }
    let callee = match arg {
        Argument::CallExpression(call) => decorator_ident_name(&call.callee),
        _ => None,
    };
    let Some(callee) = callee else {
        diagnostics.push(unsupported(
            "a non-decorator expression".to_owned(),
            "pass bellows decorator calls or other composites",
        ));
        return None;
    };
    match callee.as_str() {
        "Middleware" | "Authn" | "Authz" | "Tag" | "Security" | "Returns" => {}
        "Controller" | "Get" | "Post" | "Put" | "Delete" | "Patch" | "Sse" | "Deprecated" => {
            diagnostics.push(unsupported(
                format!("@{callee}"),
                "@Controller, route decorators, and @Deprecated go on the class or method itself",
            ));
            return None;
        }
        _ => {
            diagnostics.push(unsupported(
                format!("@{callee}"),
                "pass bellows decorator calls or other composites",
            ));
            return None;
        }
    }
    let expr = arg.as_expression()?;
    match classify_decorator(expr, import_map, display_file, diagnostics) {
        Some(DecoratorItem::Returns(r)) if r.schema.is_some() => {
            diagnostics.push(unsupported(
                format!("@Returns({}, schema)", r.status),
                "a composite's @Returns takes a status only; declare schema responses on the route",
            ));
            None
        }
        Some(item) => Some(Part::Item(item)),
        None => {
            diagnostics.push(unsupported(
                format!("@{callee} with an argument that isn't a literal"),
                "@Tag and @Security take a string literal; @Returns takes a numeric status",
            ));
            None
        }
    }
}

/// `BellowsComposite` as a type reference or as `import("…").BellowsComposite`.
fn is_composite_type(ty: &TSType<'_>) -> bool {
    match ty {
        TSType::TSTypeReference(r) => match &r.type_name {
            oxc_ast::ast::TSTypeName::IdentifierReference(id) => id.name == "BellowsComposite",
            oxc_ast::ast::TSTypeName::QualifiedName(q) => q.right.name == "BellowsComposite",
            oxc_ast::ast::TSTypeName::ThisExpression(_) => false,
        },
        TSType::TSImportType(i) => match &i.qualifier {
            Some(TSImportTypeQualifier::Identifier(id)) => id.name == "BellowsComposite",
            Some(TSImportTypeQualifier::QualifiedName(q)) => q.right.name == "BellowsComposite",
            None => false,
        },
        _ => false,
    }
}

/// A ref declared in the definition file has `origin: None` there, which
/// codegen reads as "declared in the controller file".
fn rebase_origins(item: DecoratorItem, def_file: &Path, use_file: &Path) -> DecoratorItem {
    if def_file == use_file {
        return item;
    }
    let rebase = |origin: Option<ImportOrigin>| {
        origin.or_else(|| Some(ImportOrigin::Relative(def_file.to_path_buf())))
    };
    match item {
        DecoratorItem::Middleware(refs) => DecoratorItem::Middleware(
            refs.into_iter()
                .map(|mut r| {
                    r.origin = rebase(r.origin);
                    r
                })
                .collect(),
        ),
        DecoratorItem::Authn(refs) => DecoratorItem::Authn(
            refs.into_iter()
                .map(|mut r| {
                    r.origin = rebase(r.origin);
                    r
                })
                .collect(),
        ),
        DecoratorItem::Authz(refs) => DecoratorItem::Authz(
            refs.into_iter()
                .map(|mut r| {
                    r.origin = rebase(r.origin);
                    r
                })
                .collect(),
        ),
        other => other,
    }
}

fn typed_only_diagnostic(name: &str, def_file: &Path, display_file: &str) -> ParseDiagnostic {
    ParseDiagnostic {
        file: display_file.to_owned(),
        decorator: name.to_owned(),
        summary: "is a BellowsComposite whose contents can't be read".to_owned(),
        found: format!(
            "`{name}` in {} has no `bellowsCompose(...)` initializer",
            def_file.display()
        ),
        hint: format!(
            "define it as `const {name} = bellowsCompose(...)` in TypeScript source the build \
             can read, not only in a .d.ts or through a helper function"
        ),
    }
}

fn not_a_composite_diagnostic(name: &str, display_file: &str) -> ParseDiagnostic {
    ParseDiagnostic {
        file: display_file.to_owned(),
        decorator: "bellowsCompose".to_owned(),
        summary: "argument is not a composite".to_owned(),
        found: format!("identifier `{name}`"),
        hint: "a bare identifier argument must name a `bellowsCompose(...)` const".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::{parse_entry, ControllerFile, ImportOrigin, ParseDiagnostic, Route};

    const BELLOWS: &str = r#"import { Controller, Get, Middleware, Authn, Authz, Tag, Security, Returns, bellowsCompose } from "@anvil-di/bellows";"#;

    struct Project {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Project {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(dir.path()).unwrap();
            Self { _dir: dir, root }
        }

        fn write(&self, rel: &str, content: &str) -> &Self {
            let path = self.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
            self
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel)
        }

        fn parse(&self) -> (Vec<ControllerFile>, Vec<ParseDiagnostic>) {
            parse_entry(&self.root.join("src")).unwrap()
        }
    }

    fn only_route(files: &[ControllerFile]) -> &Route {
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].controllers.len(), 1);
        assert_eq!(files[0].controllers[0].routes.len(), 1);
        &files[0].controllers[0].routes[0]
    }

    fn middleware_names(route: &Route) -> Vec<&str> {
        route.middleware.iter().map(|m| m.name.as_str()).collect()
    }

    fn controller(decorators: &str, imports: &str) -> String {
        format!(
            "{BELLOWS}\n{imports}\n\n{decorators}\n@Controller(\"/c\")\nexport class C {{\n  @Get(\"/\")\n  get() {{}}\n}}\n"
        )
    }

    #[test]
    fn same_file_composite_expands_in_place_at_both_sites() {
        let p = Project::new();
        p.write(
            "src/c.controller.ts",
            &format!(
                r#"{BELLOWS}
const first = () => {{}};
const second = () => {{}};
const third = () => {{}};
const Shared = bellowsCompose(Middleware(second), Tag("shared"));
const RouteLevel = bellowsCompose(Middleware(third), Returns(401));

@Middleware(first)
@Shared
@Controller("/c")
export class C {{
  @Get("/")
  @RouteLevel
  get() {{}}
}}
"#
            ),
        );
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        let route = only_route(&files);
        assert_eq!(middleware_names(route), ["first", "second", "third"]);
        assert!(route.middleware.iter().all(|m| m.origin.is_none()));
        assert_eq!(files[0].controllers[0].tags, ["shared"]);
        assert_eq!(route.extra_responses.len(), 1);
        assert_eq!(route.extra_responses[0].status, 401);
    }

    #[test]
    fn imported_composite_rebases_origins_to_its_definition_file() {
        let p = Project::new();
        p.write(
            "src/policies/admin.ts",
            &format!(
                r#"{BELLOWS}
import {{ audit }} from "./audit";
import rateLimit from "express-rate-limit";
export const local = () => {{}};
export const AdminRoute = bellowsCompose(Middleware(local, audit, rateLimit));
"#
            ),
        )
        .write("src/policies/audit.ts", "export const audit = () => {};\n")
        .write(
            "src/c.controller.ts",
            &controller(
                "@AdminRoute",
                r#"import { AdminRoute as Admin, AdminRoute } from "./policies/admin";"#,
            ),
        );
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        let route = only_route(&files);
        let origins: Vec<_> = route.middleware.iter().map(|m| m.origin.clone()).collect();
        assert_eq!(
            origins,
            [
                Some(ImportOrigin::Relative(p.path("src/policies/admin.ts"))),
                Some(ImportOrigin::Relative(p.path("src/policies/audit.ts"))),
                Some(ImportOrigin::Package("express-rate-limit".into())),
            ]
        );
    }

    #[test]
    fn aliased_import_resolves_by_exported_name() {
        let p = Project::new();
        p.write(
            "src/policies.ts",
            &format!("{BELLOWS}\nconst mw = () => {{}};\nexport const AdminRoute = bellowsCompose(Middleware(mw));\n"),
        )
        .write(
            "src/c.controller.ts",
            &controller("@Admin", r#"import { AdminRoute as Admin } from "./policies";"#),
        );
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(middleware_names(only_route(&files)), ["mw"]);
    }

    #[test]
    fn nested_composites_expand_depth_first_across_files() {
        let p = Project::new();
        p.write(
            "src/base.ts",
            &format!("{BELLOWS}\nexport const a = () => {{}};\nexport const Base = bellowsCompose(Middleware(a));\n"),
        )
        .write(
            "src/admin.ts",
            &format!(
                "{BELLOWS}\nimport {{ Base }} from \"./base\";\nexport const b = () => {{}};\nexport const c = () => {{}};\nexport const Admin = bellowsCompose(Middleware(b), Base, Middleware(c));\n"
            ),
        )
        .write("src/c.controller.ts", &controller("@Admin", r#"import { Admin } from "./admin";"#));
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        let route = only_route(&files);
        assert_eq!(middleware_names(route), ["b", "a", "c"]);
        assert_eq!(
            route.middleware[1].origin,
            Some(ImportOrigin::Relative(p.path("src/base.ts")))
        );
    }

    #[test]
    fn barrels_are_followed() {
        let p = Project::new();
        p.write(
            "src/policies/admin.ts",
            &format!("{BELLOWS}\nconst a = () => {{}};\nexport const Admin = bellowsCompose(Middleware(a));\n"),
        )
        .write(
            "src/policies/user.ts",
            &format!("{BELLOWS}\nconst u = () => {{}};\nconst User = bellowsCompose(Middleware(u));\nexport {{ User as Member }};\n"),
        )
        .write(
            "src/policies/index.ts",
            "export { Admin } from \"./admin\";\nexport * from \"./user\";\n",
        )
        .write(
            "src/c.controller.ts",
            &format!(
                "{BELLOWS}\nimport {{ Admin, Member }} from \"./policies\";\n\n@Admin\n@Controller(\"/c\")\nexport class C {{\n  @Get(\"/\")\n  @Member\n  get() {{}}\n}}\n"
            ),
        );
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(middleware_names(only_route(&files)), ["a", "u"]);
    }

    #[test]
    fn tsconfig_path_aliases_resolve() {
        let p = Project::new();
        p.write(
            "tsconfig.json",
            r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@policies/*": ["src/policies/*"] } } }"#,
        )
        .write(
            "src/policies/admin.ts",
            &format!("{BELLOWS}\nconst a = () => {{}};\nexport const Admin = bellowsCompose(Middleware(a));\n"),
        )
        .write("src/c.controller.ts", &controller("@Admin", r#"import { Admin } from "@policies/admin";"#));
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(middleware_names(only_route(&files)), ["a"]);
    }

    #[test]
    fn package_source_composite_expands() {
        let p = Project::new();
        p.write(
            "node_modules/acme-policies/package.json",
            r#"{ "name": "acme-policies", "exports": { ".": { "types": "./src/index.ts", "import": "./dist/index.js" } } }"#,
        )
        .write(
            "node_modules/acme-policies/src/index.ts",
            &format!("{BELLOWS}\nexport const a = () => {{}};\nexport const Admin = bellowsCompose(Middleware(a));\n"),
        )
        .write("src/c.controller.ts", &controller("@Admin", r#"import { Admin } from "acme-policies";"#));
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        let route = only_route(&files);
        assert_eq!(middleware_names(route), ["a"]);
        assert_eq!(
            route.middleware[0].origin,
            Some(ImportOrigin::Relative(
                p.path("node_modules/acme-policies/src/index.ts")
            ))
        );
    }

    #[test]
    fn declaration_only_composite_is_a_diagnostic() {
        let p = Project::new();
        p.write(
            "node_modules/acme-policies/package.json",
            r#"{ "name": "acme-policies", "types": "./dist/index.d.ts", "main": "./dist/index.js" }"#,
        )
        .write(
            "node_modules/acme-policies/dist/index.d.ts",
            "export declare const Admin: import(\"@anvil-di/bellows\").BellowsComposite;\n",
        )
        .write("src/c.controller.ts", &controller("@Admin", r#"import { Admin } from "acme-policies";"#));
        let (files, diags) = p.parse();
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].decorator, "Admin");
        assert!(diags[0].summary.contains("can't be read"));
        assert!(only_route(&files).middleware.is_empty());
    }

    #[test]
    fn annotated_helper_composite_is_a_diagnostic() {
        let p = Project::new();
        p.write(
            "src/c.controller.ts",
            &controller(
                "@Admin",
                "import type { BellowsComposite } from \"@anvil-di/bellows\";\ndeclare function makePolicy(): BellowsComposite;\nconst Admin: BellowsComposite = makePolicy();",
            ),
        );
        let (_, diags) = p.parse();
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].decorator, "Admin");
    }

    #[test]
    fn ordinary_bare_decorators_are_left_alone() {
        let p = Project::new();
        p.write("src/log.ts", "export function Logged(t: unknown) { return t; }\nexport const Traced = (t: unknown) => t;\n")
            .write(
                "src/c.controller.ts",
                &controller(
                    "@Singleton\n@Logged\n@Traced\n@Unbound",
                    "import { Singleton } from \"@anvil-di/anvil\";\nimport { Logged, Traced } from \"./log\";",
                ),
            );
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        assert!(only_route(&files).middleware.is_empty());
    }

    #[test]
    fn misplaced_items_are_dropped_with_a_diagnostic() {
        let p = Project::new();
        p.write(
            "src/c.controller.ts",
            &format!(
                r#"{BELLOWS}
const mw = () => {{}};
const Policy = bellowsCompose(Middleware(mw), Tag("t"), Returns(403));

@Policy
@Controller("/c")
export class C {{
  @Get("/")
  @Policy
  get() {{}}
}}
"#
            ),
        );
        let (files, diags) = p.parse();
        let found: Vec<_> = diags.iter().map(|d| d.found.as_str()).collect();
        assert_eq!(
            found,
            ["@Returns on a controller class", "@Tag on a route method"]
        );
        let route = only_route(&files);
        assert_eq!(middleware_names(route), ["mw", "mw"]);
        assert_eq!(files[0].controllers[0].tags, ["t"]);
        assert_eq!(route.extra_responses.len(), 1);
    }

    #[test]
    fn unsupported_arguments_are_reported_once_per_definition() {
        let p = Project::new();
        p.write(
            "src/policies.ts",
            &format!(
                r#"{BELLOWS}
const Schema = {{}};
const tag = "x";
const mw = () => {{}};
export const Bad = bellowsCompose(Get("/"), Returns(401, Schema), Tag(tag), mw, Middleware(mw));
"#
            ),
        )
        .write(
            "src/a.controller.ts",
            &controller("@Bad", r#"import { Bad } from "./policies";"#),
        )
        .write(
            "src/b.controller.ts",
            &controller("@Bad", r#"import { Bad } from "./policies";"#)
                .replace("class C", "class D"),
        );
        let (files, diags) = p.parse();
        let found: Vec<_> = diags.iter().map(|d| d.found.as_str()).collect();
        assert_eq!(
            found,
            [
                "@Get",
                "@Returns(401, schema)",
                "@Tag with an argument that isn't a literal",
                "identifier `mw`",
            ]
        );
        assert!(diags.iter().all(|d| d.file.ends_with("policies.ts")));
        for f in &files {
            assert_eq!(middleware_names(&f.controllers[0].routes[0]), ["mw"]);
        }
    }

    #[test]
    fn cycles_are_reported_and_terminate() {
        let p = Project::new();
        p.write(
            "src/a.ts",
            &format!("{BELLOWS}\nimport {{ B }} from \"./b\";\nconst x = () => {{}};\nexport const A = bellowsCompose(Middleware(x), B);\n"),
        )
        .write(
            "src/b.ts",
            &format!("{BELLOWS}\nimport {{ A }} from \"./a\";\nexport const B = bellowsCompose(A);\n"),
        )
        .write("src/c.controller.ts", &controller("@A", r#"import { A } from "./a";"#));
        let (files, diags) = p.parse();
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].found, "A → B → A");
        assert_eq!(middleware_names(only_route(&files)), ["x"]);
    }

    #[test]
    fn authn_scheme_resolves_through_a_composite() {
        let p = Project::new();
        p.write(
            "src/auth/session.ts",
            "import type { AuthnService, AuthnResult } from \"@anvil-di/bellows\";\nexport class SessionAuthn implements AuthnService<unknown, \"sessionAuth\"> {\n  identify(): AuthnResult<unknown> { return { identified: false }; }\n}\n",
        )
        .write(
            "src/auth/policies.ts",
            &format!("{BELLOWS}\nimport {{ SessionAuthn }} from \"./session\";\nexport const Authenticated = bellowsCompose(Authn(SessionAuthn));\n"),
        )
        .write(
            "src/c.controller.ts",
            &controller("@Authenticated", r#"import { Authenticated } from "./auth/policies";"#),
        );
        let (files, diags) = p.parse();
        assert!(diags.is_empty(), "{diags:?}");
        let route = only_route(&files);
        assert_eq!(route.authn.len(), 1);
        assert_eq!(route.authn[0].scheme.as_deref(), Some("sessionAuth"));
        assert_eq!(
            route.authn[0].origin,
            Some(ImportOrigin::Relative(p.path("src/auth/session.ts")))
        );
    }

    #[test]
    fn composite_used_from_its_own_file_keeps_same_file_origins() {
        let p = Project::new();
        p.write(
            "src/c.controller.ts",
            &controller(
                "@Local",
                "const mw = () => {};\nconst Local = bellowsCompose(Middleware(mw));",
            ),
        );
        let (files, _) = p.parse();
        assert_eq!(only_route(&files).middleware[0].origin, None);
        assert!(Path::new(&p.path("src/c.controller.ts")).is_file());
    }
}
