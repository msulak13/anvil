//! Decorator extractor: lower a parsed Oxc AST into [`anvil_core::ir`].
//!
//! Recognized decorators (TC39 Stage-3):
//!
//! - `@Module` — class-level. Marks the class as a module hosting `@Provides` and/or `@Binds` methods.
//! - `@Provides` — method-level on a `@Module` class. Must be `static` and have an explicit return type.
//! - `@Binds` — method-level on a `@Module` class. Must be `static`, take exactly one parameter, and have an explicit return type. The codegen emits a factory that delegates to the parameter's binding.
//! - `@Inject` — class-level on any class. The class becomes a self-binding whose deps are the constructor's parameter types. (Stage-3 decorators don't apply to constructors, so the legacy `@Inject constructor(...)` placement is rejected with [`ExtractError::InjectOnConstructor`].)
//! - `@Component(config)` — class-level on an abstract class. `config.modules` is an array of class identifiers.
//! - `@Subcomponent(config)` — class-level on an abstract class. Same shape as `@Component`; the parent component exposes child subcomponent factories via abstract zero-arg methods whose return type names a `@Subcomponent` class.
//! - `@Singleton` — class-level. Sets the binding's scope.
//!
//! See `docs/ir.md` for the IR types this module emits.

use std::collections::HashSet;

use oxc_ast::ast::{
    Argument, ClassElement, Declaration, Decorator, Expression, FormalParameter,
    MethodDefinitionKind, ObjectPropertyKind, PropertyKey, Statement, TSLiteral, TSType,
    TSTypeAnnotation,
};
use oxc_span::Span;
use anvil_core::ir::{
    Binding, ClassRef, ComponentDecl, EntryPoint, Key, ModuleDecl, ModulePath, MultibindRole,
    ParsedFile, Provider, Scope, SourceSpan, SubcomponentDecl,
};

use crate::imports::{ImportMap, ImportSource};

/// All ways the decorator extractor can fail.
///
/// In M1 these errors are reported flat (no source spans rendered yet).
/// M3 wires them into `miette` diagnostics with caret annotations.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// `@Provides` was placed on a non-`static` method.
    #[error("@Provides on '{method}' in module '{module}' must be a static method")]
    ProvidesNotStatic {
        /// Containing module class name.
        module: String,
        /// Method name.
        method: String,
        /// Source span of the offending method.
        span: Span,
    },
    /// `@Provides` method has no explicit return type annotation.
    #[error("@Provides method '{module}.{method}' must declare an explicit return type")]
    ProvidesMissingReturnType {
        /// Containing module class name.
        module: String,
        /// Method name.
        method: String,
        /// Source span of the offending method.
        span: Span,
    },
    /// A type annotation is not a simple identifier reference.
    ///
    /// M1 only supports `T` and `T<U>`-free identifiers. Generics, unions,
    /// and complex types are rejected.
    #[error("unsupported type annotation in '{context}': only simple identifier types are supported in v0.1")]
    UnsupportedType {
        /// Where the unsupported type appeared (for the error message).
        context: String,
        /// Source span.
        span: Span,
    },
    /// `@Component` or `@Subcomponent` was used without an `({ modules: [...] })` argument.
    #[error(
        "@{kind} on '{class}' must be invoked with a config object: @{kind}({{ modules: [...] }})"
    )]
    ComponentMissingConfig {
        /// Decorator kind: `"Component"` or `"Subcomponent"`.
        kind: &'static str,
        /// Class name.
        class: String,
        /// Source span.
        span: Span,
    },
    /// `@Component({ modules: ... })` (or `@Subcomponent`) was given a non-array value.
    #[error("@{kind}({{ modules: ... }}) on '{class}' must be an array of class identifiers")]
    ComponentBadModules {
        /// Decorator kind: `"Component"` or `"Subcomponent"`.
        kind: &'static str,
        /// Class name.
        class: String,
        /// Source span.
        span: Span,
    },
    /// An abstract method on a `@Component` class lacks an explicit return type.
    #[error("entry point '{class}.{method}' must declare an explicit return type")]
    EntryPointMissingReturnType {
        /// Component class name.
        class: String,
        /// Method name.
        method: String,
        /// Source span.
        span: Span,
    },
    /// `@Inject` was placed on a constructor. TC39 Stage-3 decorators don't
    /// decorate constructors; `@Inject` must be applied to the class itself.
    #[error(
        "@Inject must be applied to the class, not its constructor (TC39 Stage-3 decorators don't \
         decorate constructors). Move `@Inject` from the constructor of `{class}` onto the class \
         declaration."
    )]
    InjectOnConstructor {
        /// Class containing the offending constructor.
        class: String,
        /// Source span of the offending constructor.
        span: Span,
    },
    /// `@Binds` was placed on a non-`static` method.
    ///
    /// TC39 Stage-3 decorators cannot decorate abstract methods (TS error 1249),
    /// so `@Binds` must be a `static` method with a trivial body (`return <param>;`).
    /// `anvil-codegen` ignores the body and emits a delegate to the target's factory.
    #[error("@Binds method '{module}.{method}' must be a static method")]
    BindsNotStatic {
        /// Containing module class name.
        module: String,
        /// Method name.
        method: String,
        /// Source span of the offending method.
        span: Span,
    },
    /// `@Binds` method has no explicit return type.
    #[error("@Binds method '{module}.{method}' must declare an explicit return type")]
    BindsMissingReturnType {
        /// Containing module class name.
        module: String,
        /// Method name.
        method: String,
        /// Source span.
        span: Span,
    },
    /// `@Binds` method has the wrong number of parameters (must be exactly one).
    #[error("@Binds method '{module}.{method}' must take exactly one parameter (got {count})")]
    BindsWrongArity {
        /// Containing module class name.
        module: String,
        /// Method name.
        method: String,
        /// Actual parameter count.
        count: usize,
        /// Source span.
        span: Span,
    },
    /// `@Binds` and `@Provides` were both placed on the same method.
    #[error("method '{module}.{method}' cannot be both @Binds and @Provides")]
    BindsAndProvides {
        /// Containing module class name.
        module: String,
        /// Method name.
        method: String,
        /// Source span.
        span: Span,
    },
    /// An async `@Provides` method's return type isn't `Promise<T>`.
    /// M12: anvil unwraps the inner type for the binding key, so the
    /// annotation must be exactly a single-argument `Promise<…>`.
    #[error("async @Provides return type in '{context}' must be `Promise<T>` for some concrete T")]
    AsyncProvidesMissingPromise {
        /// Where the offending type annotation appeared.
        context: String,
        /// Source span of the offending annotation.
        span: Span,
    },
    /// `@Inject` was placed on a class whose constructor is `async`.
    /// TypeScript doesn't permit async constructors (TS error 1107) —
    /// we surface an anvil-flavored diagnostic so the user knows to move
    /// the async work into an `@Provides` method instead.
    #[error(
        "@Inject class '{class}' has an async constructor — TypeScript doesn't permit \
         async constructors. Move the async work into an `@Provides` method instead."
    )]
    AsyncInjectCtor {
        /// Class name.
        class: String,
        /// Source span of the offending constructor.
        span: Span,
    },
    /// `Token<T, "name">` was written with a non-string-literal second type argument.
    ///
    /// The codegen requires a statically-known name (a string literal) to mint the
    /// `Key::Token`. A const reference like `Token<Database, MY_TOKEN>` can't be
    /// resolved without a type checker, so it's rejected here.
    #[error(
        "`Token<T, Name>` in '{context}' requires a string literal for Name, \
         e.g. `Token<Database, \"primary-db\">`. Non-literal names (const references, \
         computed types) are not supported."
    )]
    TokenNonLiteralName {
        /// Where the annotation appeared.
        context: String,
        /// Source span.
        span: Span,
    },
    /// `@IntoSet` was placed on something other than a `@Provides` method
    /// (in v0.1, e.g. a `@Binds` method or a method with no provider decorator).
    #[error("@IntoSet on '{module}.{method}' is only supported on @Provides methods in v0.1")]
    IntoSetWithoutProvides {
        /// Containing module class name.
        module: String,
        /// Method name.
        method: String,
        /// Source span.
        span: Span,
    },
}

/// Result of extraction.
pub type Result<T> = std::result::Result<T, ExtractError>;

/// Names recognized as decorators in v0.1. Bound via the user's `import { … } from "@msulak/anvil"`.
const KNOWN_DECORATOR_NAMES: &[&str] = &[
    "Module",
    "Provides",
    "Binds",
    "Inject",
    "Component",
    "Subcomponent",
    "Singleton",
    "IntoSet",
];

/// Convert an `oxc_span::Span` into the parser-agnostic `SourceSpan` carried
/// in the IR.
fn to_ir_span(file_path: &str, span: Span) -> SourceSpan {
    SourceSpan::new(file_path.to_owned(), span.start, span.end)
}

/// Walk a parsed program and produce a [`ParsedFile`].
#[allow(clippy::too_many_lines)]
pub fn extract(
    program: &oxc_ast::ast::Program<'_>,
    imports: &ImportMap,
    local_classes: &[String],
    file_path: &str,
) -> Result<ParsedFile> {
    let local_set: HashSet<&str> = local_classes.iter().map(String::as_str).collect();
    let mut out = ParsedFile {
        path: file_path.to_owned(),
        ..Default::default()
    };

    for stmt in &program.body {
        let class = match stmt {
            Statement::ClassDeclaration(c) => Some(c.as_ref()),
            Statement::ExportNamedDeclaration(decl) => match &decl.declaration {
                Some(Declaration::ClassDeclaration(c)) => Some(c.as_ref()),
                _ => None,
            },
            _ => None,
        };
        let Some(class) = class else { continue };
        let Some(class_ident) = &class.id else {
            continue;
        };
        let class_name = class_ident.name.as_str();
        let class_ref = ClassRef {
            module: ModulePath::same_file(),
            name: class_name.to_owned(),
        };
        let class_span = to_ir_span(file_path, class.span);

        let class_decorators = collect_decorator_kinds(&class.decorators);

        // Track the class-level scope so it applies to a class's @Inject ctor binding.
        let scope = if class_decorators.iter().any(|d| d.name == "Singleton") {
            Scope::Singleton
        } else {
            Scope::Unscoped
        };

        // 1. @Module
        if class_decorators.iter().any(|d| d.name == "Module") {
            let provides =
                extract_provides(&class.body.body, &class_ref, imports, &local_set, file_path)?;
            out.modules.push(ModuleDecl {
                class: class_ref.clone(),
                provides,
                source: class_span.clone(),
            });
        }

        // 2. @Component
        if let Some(component_dec) = class_decorators.iter().find(|d| d.name == "Component") {
            let modules = parse_component_modules(
                component_dec,
                "Component",
                class_name,
                imports,
                &local_set,
            )?;
            let entry_points =
                extract_entry_points(&class.body.body, class_name, imports, &local_set, file_path)?;
            out.components.push(ComponentDecl {
                class: class_ref.clone(),
                modules,
                scope,
                entry_points,
                source: class_span.clone(),
            });
        }

        // 2b. @Subcomponent — same shape as @Component, separate IR slot.
        if let Some(sub_dec) = class_decorators.iter().find(|d| d.name == "Subcomponent") {
            let modules =
                parse_component_modules(sub_dec, "Subcomponent", class_name, imports, &local_set)?;
            let entry_points =
                extract_entry_points(&class.body.body, class_name, imports, &local_set, file_path)?;
            out.subcomponents.push(SubcomponentDecl {
                class: class_ref.clone(),
                modules,
                scope,
                entry_points,
                source: class_span.clone(),
            });
        }

        // 3. @Inject (class-level) → self-binding whose deps come from the constructor params.
        //
        // Reject the legacy `@Inject constructor(...)` placement loudly: Stage-3
        // decorators don't decorate constructors, so accepting it would generate
        // user source that `tsc` rejects.
        if let Some(ctor_span) = constructor_with_inject_decorator(&class.body.body) {
            return Err(ExtractError::InjectOnConstructor {
                class: class_name.to_owned(),
                span: ctor_span,
            });
        }
        if class_decorators.iter().any(|d| d.name == "Inject") {
            // M12: an async constructor isn't valid TS (TS error 1107)
            // and anvil has no way to await one. Reject early with a
            // helpful diagnostic that points the user at @Provides.
            if let Some(span) = constructor_async_span(&class.body.body) {
                return Err(ExtractError::AsyncInjectCtor {
                    class: class_name.to_owned(),
                    span,
                });
            }
            let (binding_span, params) = find_constructor(&class.body.body)
                .map_or((class.span, &[][..]), |(span, params)| (span, params));
            let deps = params_to_keys(params, class_name, "constructor", imports, &local_set)?;
            out.inject_classes.push(Binding {
                key: Key::class(ModulePath::same_file(), class_name),
                provider: Provider::InjectCtor {
                    class: class_ref.clone(),
                },
                scope,
                deps,
                source: to_ir_span(file_path, binding_span),
                role: MultibindRole::None,
                token_inner_type: None,
            });
        }
    }

    Ok(out)
}

/// A decorator we recognized by name.
struct DecoratorRef<'a> {
    name: &'a str,
    decorator: &'a Decorator<'a>,
}

fn collect_decorator_kinds<'a>(decorators: &'a [Decorator<'a>]) -> Vec<DecoratorRef<'a>> {
    let mut out = Vec::new();
    for d in decorators {
        if let Some(name) = decorator_name(d) {
            if KNOWN_DECORATOR_NAMES.contains(&name) {
                out.push(DecoratorRef { name, decorator: d });
            }
        }
    }
    out
}

/// Return the bare identifier name for a decorator like `@Module` or
/// `@Component(...)`. Returns `None` for member-expression decorators
/// (`@some.thing`), which v0.1 does not support.
fn decorator_name<'a>(d: &'a Decorator<'a>) -> Option<&'a str> {
    match &d.expression {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::CallExpression(call) => match &call.callee {
            Expression::Identifier(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn extract_provides(
    body: &[ClassElement<'_>],
    module_class: &ClassRef,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
    file_path: &str,
) -> Result<Vec<Binding>> {
    let mut out = Vec::new();
    for member in body {
        let ClassElement::MethodDefinition(m) = member else {
            continue;
        };
        let decorator_names: Vec<&str> = m.decorators.iter().filter_map(decorator_name).collect();
        let has_provides = decorator_names.contains(&"Provides");
        let has_binds = decorator_names.contains(&"Binds");
        let has_into_set = decorator_names.contains(&"IntoSet");
        if !has_provides && !has_binds {
            continue;
        }
        let method_name =
            property_key_name(&m.key).map_or_else(|| "<computed>".to_owned(), str::to_owned);

        if has_provides && has_binds {
            return Err(ExtractError::BindsAndProvides {
                module: module_class.name.clone(),
                method: method_name,
                span: m.span,
            });
        }

        // M9: @IntoSet is only supported on @Provides methods in v0.1.
        if has_into_set && !has_provides {
            return Err(ExtractError::IntoSetWithoutProvides {
                module: module_class.name.clone(),
                method: method_name,
                span: m.span,
            });
        }

        let scope = if decorator_names.contains(&"Singleton") {
            Scope::Singleton
        } else {
            Scope::Unscoped
        };

        let role = if has_into_set {
            MultibindRole::IntoSet
        } else {
            MultibindRole::None
        };

        let binding = if has_binds {
            extract_binds_method(
                m,
                module_class,
                &method_name,
                scope,
                imports,
                local_classes,
                file_path,
            )?
        } else {
            extract_provides_method(
                m,
                module_class,
                &method_name,
                scope,
                role,
                imports,
                local_classes,
                file_path,
            )?
        };
        out.push(binding);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn extract_provides_method(
    m: &oxc_ast::ast::MethodDefinition<'_>,
    module_class: &ClassRef,
    method_name: &str,
    scope: Scope,
    role: MultibindRole,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
    file_path: &str,
) -> Result<Binding> {
    if !m.r#static {
        return Err(ExtractError::ProvidesNotStatic {
            module: module_class.name.clone(),
            method: method_name.to_owned(),
            span: m.span,
        });
    }

    let return_ann =
        m.value
            .return_type
            .as_deref()
            .ok_or_else(|| ExtractError::ProvidesMissingReturnType {
                module: module_class.name.clone(),
                method: method_name.to_owned(),
                span: m.span,
            })?;

    // M12: an `async` @Provides method has return type `Promise<T>` —
    // unwrap to T for the binding's key. The dagger awaits the call
    // during its `_resolve` phase and consumers see only the resolved
    // value. Sync @Provides keep the old behavior.
    let is_async = m.value.r#async;
    let context = format!(
        "@Provides {}.{} return type",
        module_class.name, method_name
    );
    let key = if is_async {
        promise_unwrap_to_key(return_ann, &context, imports, local_classes)?
    } else {
        type_annotation_to_key(return_ann, &context, imports, local_classes)?
    };

    // M14: extract the inner type string for Token<T, "name"> bindings so
    // codegen can emit the correct TS type annotation and cast.
    let token_inner_type = if matches!(key, Key::Token { .. }) {
        extract_token_inner_type(return_ann)
    } else {
        None
    };

    let deps = params_to_keys(
        &m.value.params.items,
        &module_class.name,
        method_name,
        imports,
        local_classes,
    )?;
    Ok(Binding {
        key,
        provider: Provider::ProvidesMethod {
            module: module_class.clone(),
            method: method_name.to_owned(),
            is_async,
        },
        scope,
        deps,
        source: to_ir_span(file_path, m.span),
        role,
        token_inner_type,
    })
}

/// Unwrap a `Promise<T>` type annotation into its inner key. Used for
/// async `@Provides` methods (M12) — the binding's identity is the
/// resolved type, not `Promise<T>`.
fn promise_unwrap_to_key(
    ann: &TSTypeAnnotation<'_>,
    context: &str,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
) -> Result<Key> {
    let TSType::TSTypeReference(tref) = &ann.type_annotation else {
        return Err(ExtractError::AsyncProvidesMissingPromise {
            context: context.to_owned(),
            span: ann.span,
        });
    };
    let oxc_ast::ast::TSTypeName::IdentifierReference(id) = &tref.type_name else {
        return Err(ExtractError::AsyncProvidesMissingPromise {
            context: context.to_owned(),
            span: ann.span,
        });
    };
    if id.name.as_str() != "Promise" {
        return Err(ExtractError::AsyncProvidesMissingPromise {
            context: context.to_owned(),
            span: ann.span,
        });
    }
    let Some(args) = tref.type_arguments.as_deref() else {
        return Err(ExtractError::AsyncProvidesMissingPromise {
            context: context.to_owned(),
            span: ann.span,
        });
    };
    if args.params.len() != 1 {
        return Err(ExtractError::AsyncProvidesMissingPromise {
            context: context.to_owned(),
            span: ann.span,
        });
    }
    let inner = args.params.first().expect("len 1");
    ts_type_to_key(inner, context, imports, local_classes, ann.span)
}

fn extract_binds_method(
    m: &oxc_ast::ast::MethodDefinition<'_>,
    module_class: &ClassRef,
    method_name: &str,
    scope: Scope,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
    file_path: &str,
) -> Result<Binding> {
    if !m.r#static {
        return Err(ExtractError::BindsNotStatic {
            module: module_class.name.clone(),
            method: method_name.to_owned(),
            span: m.span,
        });
    }
    let return_ann =
        m.value
            .return_type
            .as_deref()
            .ok_or_else(|| ExtractError::BindsMissingReturnType {
                module: module_class.name.clone(),
                method: method_name.to_owned(),
                span: m.span,
            })?;
    let params = m.value.params.items.as_slice();
    if params.len() != 1 {
        return Err(ExtractError::BindsWrongArity {
            module: module_class.name.clone(),
            method: method_name.to_owned(),
            count: params.len(),
            span: m.span,
        });
    }
    let key = type_annotation_to_key(
        return_ann,
        &format!("@Binds {}.{} return type", module_class.name, method_name),
        imports,
        local_classes,
    )?;
    let target_keys = params_to_keys(
        params,
        &module_class.name,
        method_name,
        imports,
        local_classes,
    )?;
    let target = target_keys.into_iter().next().expect("arity-checked above");
    Ok(Binding {
        key,
        provider: Provider::Binds {
            target: target.clone(),
        },
        scope,
        deps: vec![target],
        source: to_ir_span(file_path, m.span),
        role: MultibindRole::None,
        token_inner_type: None,
    })
}

fn extract_entry_points(
    body: &[ClassElement<'_>],
    class_name: &str,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
    file_path: &str,
) -> Result<Vec<EntryPoint>> {
    let mut out = Vec::new();
    for member in body {
        let ClassElement::MethodDefinition(m) = member else {
            continue;
        };
        if !matches!(m.kind, MethodDefinitionKind::Method) {
            continue;
        }
        // Only abstract methods are entry points.
        let MethodDefinitionKind::Method = m.kind else {
            continue;
        };
        if !is_abstract_method(m) {
            continue;
        }
        let method_name =
            property_key_name(&m.key).map_or_else(|| "<computed>".to_owned(), str::to_owned);
        let return_ann = m.value.return_type.as_deref().ok_or_else(|| {
            ExtractError::EntryPointMissingReturnType {
                class: class_name.to_owned(),
                method: method_name.clone(),
                span: m.span,
            }
        })?;
        let key = type_annotation_to_key(
            return_ann,
            &format!("entry point {class_name}.{method_name}"),
            imports,
            local_classes,
        )?;
        // M11: capture each formal parameter on the abstract method. For
        // a regular @Component entry point this is an empty list (and
        // the graph layer rejects non-empty lists with a diagnostic).
        // For @Subcomponent factory methods, each parameter becomes a
        // virtual binding inside the child graph.
        let factory_params = extract_entry_factory_params(
            m,
            class_name,
            &method_name,
            imports,
            local_classes,
            file_path,
        )?;
        out.push(EntryPoint {
            name: method_name,
            key,
            source: to_ir_span(file_path, m.span),
            factory_params,
        });
    }
    Ok(out)
}

/// Lower an entry-point method's formal parameters into [`FactoryParam`]s.
///
/// Each parameter must carry an explicit type annotation; an annotation
/// is required everywhere else in anvil (no inference from a TS type
/// checker), and entry points are no exception. The parser surfaces a
/// missing annotation as [`ExtractError::UnsupportedType`] via the
/// existing `params_to_keys` plumbing — repurposing that error keeps the
/// surface area small.
fn extract_entry_factory_params(
    m: &oxc_ast::ast::MethodDefinition<'_>,
    class_name: &str,
    method_name: &str,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
    file_path: &str,
) -> Result<Vec<anvil_core::ir::FactoryParam>> {
    let params = m.value.params.items.as_slice();
    if params.is_empty() {
        return Ok(Vec::new());
    }
    let keys = params_to_keys(params, class_name, method_name, imports, local_classes)?;
    let mut out = Vec::with_capacity(params.len());
    for (idx, (p, key)) in params.iter().zip(keys).enumerate() {
        let name = formal_param_name(p).map_or_else(|| format!("_arg{idx}"), str::to_owned);
        out.push(anvil_core::ir::FactoryParam {
            name,
            key,
            source: to_ir_span(file_path, p.span),
        });
    }
    Ok(out)
}

/// Extract the bare identifier name of a formal parameter
/// (`(req: Request) => …` → `"req"`). Destructuring patterns and
/// rest/array bindings are not supported on entry-point methods in v0.1
/// and surface as `None`, which `extract_entry_factory_params` falls
/// back to a synthetic `_argN` for.
fn formal_param_name<'a>(p: &'a FormalParameter<'a>) -> Option<&'a str> {
    use oxc_ast::ast::BindingPattern;
    match &p.pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        // Destructuring / array / assignment patterns aren't supported
        // as factory parameter names — the dagger can't synthesize a
        // sensible field for `({ req, res }) => …`. The caller falls
        // back to `_argN`.
        _ => None,
    }
}

fn is_abstract_method(m: &oxc_ast::ast::MethodDefinition<'_>) -> bool {
    matches!(
        m.r#type,
        oxc_ast::ast::MethodDefinitionType::TSAbstractMethodDefinition
    )
}

/// Locate the constructor on a class, returning its span and parameter list.
///
/// Returns `None` for classes with no explicit constructor (treated as a
/// no-arg ctor — i.e. zero deps).
fn find_constructor<'a>(body: &'a [ClassElement<'a>]) -> Option<(Span, &'a [FormalParameter<'a>])> {
    for member in body {
        let ClassElement::MethodDefinition(m) = member else {
            continue;
        };
        if matches!(m.kind, MethodDefinitionKind::Constructor) {
            return Some((m.span, m.value.params.items.as_slice()));
        }
    }
    None
}

/// If the class has an `async` constructor, return its span. M12 uses
/// this to reject async-on-`@Inject` with an anvil-flavored diagnostic
/// (TypeScript would also flag TS1107, but our error names the right
/// remediation: move the async work into a `@Provides`).
fn constructor_async_span(body: &[ClassElement<'_>]) -> Option<Span> {
    for member in body {
        let ClassElement::MethodDefinition(m) = member else {
            continue;
        };
        if matches!(m.kind, MethodDefinitionKind::Constructor) && m.value.r#async {
            return Some(m.span);
        }
    }
    None
}

/// If a constructor has `@Inject` (legacy placement), return its span so
/// the caller can raise [`ExtractError::InjectOnConstructor`].
fn constructor_with_inject_decorator(body: &[ClassElement<'_>]) -> Option<Span> {
    for member in body {
        let ClassElement::MethodDefinition(m) = member else {
            continue;
        };
        if !matches!(m.kind, MethodDefinitionKind::Constructor) {
            continue;
        }
        if m.decorators
            .iter()
            .filter_map(decorator_name)
            .any(|n| n == "Inject")
        {
            return Some(m.span);
        }
    }
    None
}

fn params_to_keys(
    params: &[FormalParameter<'_>],
    class_name: &str,
    method_name: &str,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
) -> Result<Vec<Key>> {
    let mut out = Vec::with_capacity(params.len());
    for (idx, p) in params.iter().enumerate() {
        let context = format!("{class_name}.{method_name} parameter {idx}");
        let ann = p
            .type_annotation
            .as_deref()
            .ok_or(ExtractError::UnsupportedType {
                context: context.clone(),
                span: p.span,
            })?;
        let key = type_annotation_to_key(ann, &context, imports, local_classes)?;
        out.push(key);
    }
    Ok(out)
}

fn type_annotation_to_key(
    ann: &TSTypeAnnotation<'_>,
    context: &str,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
) -> Result<Key> {
    let TSType::TSTypeReference(tref) = &ann.type_annotation else {
        return Err(ExtractError::UnsupportedType {
            context: context.to_owned(),
            span: ann.span,
        });
    };
    let oxc_ast::ast::TSTypeName::IdentifierReference(id) = &tref.type_name else {
        return Err(ExtractError::UnsupportedType {
            context: context.to_owned(),
            span: ann.span,
        });
    };
    let name = id.name.as_str();

    if let Some(args) = tref.type_arguments.as_deref() {
        // M9: special-case `Set<T>` as the multibinding aggregate key.
        if name == "Set" && args.params.len() == 1 {
            let inner = args.params.first().expect("len 1");
            let element = ts_type_to_key(inner, context, imports, local_classes, ann.span)?;
            return Ok(Key::Set {
                element: Box::new(element),
            });
        }

        // M14: `Token<T, "name">` → Key::Token { name }.
        // Check Token BEFORE doing resolve_name_to_key to allow a user-defined class
        // named "Token" that happens not to come from @msulak/anvil to pass through as
        // a generic Key::Class. We only apply the Token logic when the specifier resolves
        // to @msulak/anvil.
        if name == "Token" && args.params.len() >= 2 {
            // Confirm it's from @msulak/anvil by checking the import map.
            let is_anvil_token = if let Some(ImportSource { specifier, .. }) = imports.get(name) {
                specifier.contains("@msulak/anvil")
            } else {
                false
            };
            if is_anvil_token {
                if let TSType::TSLiteralType(lit) = &args.params[1] {
                    if let TSLiteral::StringLiteral(s) = &lit.literal {
                        let token_name = s.value.as_str().to_owned();
                        return Ok(Key::Token { name: token_name });
                    }
                }
                // Second arg is present but not a string literal — reject rather than
                // silently producing Key::Class { name: "Token", .. }.
                return Err(ExtractError::TokenNonLiteralName {
                    context: context.to_owned(),
                    span: ann.span,
                });
            }
        }

        // M15: generic class types → Key::Class with type_args.
        let base_key = resolve_name_to_key(name, imports, local_classes);
        let type_args: Option<Vec<Key>> = args
            .params
            .iter()
            .map(|p| ts_type_to_key(p, context, imports, local_classes, ann.span).ok())
            .collect();
        if let (Some(type_args), Key::Class { name: cls_name, module, .. }) =
            (type_args, base_key.clone())
        {
            return Ok(Key::Class {
                name: cls_name,
                module,
                type_args,
            });
        }
        return Ok(base_key);
    }
    Ok(resolve_name_to_key(name, imports, local_classes))
}

/// Lower a bare `TSType` (the element of `Set<T>` or a generic type arg) into a [`Key`].
/// Mirrors [`type_annotation_to_key`] but operates on the inner type-parameter slot
/// where there is no surrounding [`TSTypeAnnotation`].
fn ts_type_to_key(
    ty: &TSType<'_>,
    context: &str,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
    fallback_span: Span,
) -> Result<Key> {
    // Primitive keyword types are represented as synthetic `Key::Class` entries
    // with a well-known sentinel module so they participate in type_args equality
    // correctly: `Repository<string>` ≠ `Repository<number>`.
    let primitive_name = match ty {
        TSType::TSStringKeyword(_) => Some("string"),
        TSType::TSNumberKeyword(_) => Some("number"),
        TSType::TSBooleanKeyword(_) => Some("boolean"),
        _ => None,
    };
    if let Some(prim) = primitive_name {
        return Ok(Key::class(ModulePath::from_abs("<builtin>"), prim));
    }
    let TSType::TSTypeReference(tref) = ty else {
        return Err(ExtractError::UnsupportedType {
            context: context.to_owned(),
            span: fallback_span,
        });
    };
    let oxc_ast::ast::TSTypeName::IdentifierReference(id) = &tref.type_name else {
        return Err(ExtractError::UnsupportedType {
            context: context.to_owned(),
            span: fallback_span,
        });
    };
    let name = id.name.as_str();
    if let Some(args) = tref.type_arguments.as_deref() {
        if name == "Set" && args.params.len() == 1 {
            let inner = args.params.first().expect("len 1");
            let element = ts_type_to_key(inner, context, imports, local_classes, fallback_span)?;
            return Ok(Key::Set {
                element: Box::new(element),
            });
        }
        // M14: Token<T, "name"> inside a type arg position.
        if name == "Token" && args.params.len() >= 2 {
            let is_anvil_token = if let Some(ImportSource { specifier, .. }) = imports.get(name) {
                specifier.contains("@msulak/anvil")
            } else {
                false
            };
            if is_anvil_token {
                if let TSType::TSLiteralType(lit) = &args.params[1] {
                    if let TSLiteral::StringLiteral(s) = &lit.literal {
                        let token_name = s.value.as_str().to_owned();
                        return Ok(Key::Token { name: token_name });
                    }
                }
                return Err(ExtractError::TokenNonLiteralName {
                    context: context.to_owned(),
                    span: fallback_span,
                });
            }
        }
        // M15: generic class type args.
        let base_key = resolve_name_to_key(name, imports, local_classes);
        let type_args: Option<Vec<Key>> = args
            .params
            .iter()
            .map(|p| ts_type_to_key(p, context, imports, local_classes, fallback_span).ok())
            .collect();
        if let (Some(type_args), Key::Class { name: cls_name, module, .. }) =
            (type_args, base_key.clone())
        {
            return Ok(Key::Class {
                name: cls_name,
                module,
                type_args,
            });
        }
        return Ok(base_key);
    }
    Ok(resolve_name_to_key(name, imports, local_classes))
}

/// Syntactically convert a `TSType` to its string representation (no type checker).
/// Returns `None` for types that can't be represented as a simple string.
fn ts_type_to_string(ty: &TSType<'_>) -> Option<String> {
    match ty {
        TSType::TSTypeReference(r) => {
            let name = match &r.type_name {
                oxc_ast::ast::TSTypeName::IdentifierReference(id) => {
                    id.name.as_str().to_owned()
                }
                _ => return None,
            };
            if let Some(args) = r.type_arguments.as_deref() {
                let arg_strs: Vec<String> = args
                    .params
                    .iter()
                    .filter_map(|p| ts_type_to_string(p))
                    .collect();
                if arg_strs.len() == args.params.len() {
                    return Some(format!("{}<{}>", name, arg_strs.join(", ")));
                }
            }
            Some(name)
        }
        TSType::TSStringKeyword(_) => Some("string".into()),
        TSType::TSNumberKeyword(_) => Some("number".into()),
        TSType::TSBooleanKeyword(_) => Some("boolean".into()),
        TSType::TSAnyKeyword(_) => Some("any".into()),
        _ => None,
    }
}

/// Extract the inner type string for a `Token<T, "name">` annotation.
/// Returns `Some("T")` for `Token<Database, "primary-db">`, `None` otherwise.
fn extract_token_inner_type(ann: &TSTypeAnnotation<'_>) -> Option<String> {
    let TSType::TSTypeReference(r) = &ann.type_annotation else {
        return None;
    };
    let name = match &r.type_name {
        oxc_ast::ast::TSTypeName::IdentifierReference(id) => id.name.as_str(),
        _ => return None,
    };
    if name != "Token" {
        return None;
    }
    let args = r.type_arguments.as_deref()?;
    if args.params.is_empty() {
        return None;
    }
    ts_type_to_string(&args.params[0])
}

/// Look up a referenced identifier in the import map, falling back to the
/// same-file sentinel if it is declared locally.
///
/// In M1 unknown identifiers (neither imported nor declared locally) are
/// also returned as same-file refs; this is harmless because the cross-file
/// resolver in M2 will normalize all module paths anyway, and a truly
/// dangling reference will surface as `MissingBinding` in M3.
fn resolve_name_to_key(name: &str, imports: &ImportMap, local_classes: &HashSet<&str>) -> Key {
    if let Some(ImportSource {
        specifier,
        exported_name,
    }) = imports.get(name)
    {
        return Key::class(ModulePath::from_specifier(specifier.clone()), exported_name.clone());
    }
    // Both locally declared and unknown identifiers map to the SAME_FILE
    // sentinel; M2's resolver normalizes them to absolute paths and M3's
    // graph builder reports any truly dangling reference as a
    // `MissingBinding` diagnostic.
    let _ = local_classes;
    Key::class(ModulePath::same_file(), name)
}

fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

fn parse_component_modules(
    decorator: &DecoratorRef<'_>,
    kind: &'static str,
    class_name: &str,
    imports: &ImportMap,
    local_classes: &HashSet<&str>,
) -> Result<Vec<ClassRef>> {
    let Expression::CallExpression(call) = &decorator.decorator.expression else {
        return Err(ExtractError::ComponentMissingConfig {
            kind,
            class: class_name.to_owned(),
            span: decorator.decorator.span,
        });
    };

    // Allow @Component() with no args: zero modules.
    if call.arguments.is_empty() {
        return Ok(Vec::new());
    }
    let Some(Argument::ObjectExpression(obj)) = call.arguments.first() else {
        return Err(ExtractError::ComponentMissingConfig {
            kind,
            class: class_name.to_owned(),
            span: decorator.decorator.span,
        });
    };

    // Find the `modules:` property.
    let mut modules: Vec<ClassRef> = Vec::new();
    let mut saw_modules_key = false;
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key_name = property_key_name(&p.key).unwrap_or("");
        if key_name != "modules" {
            continue;
        }
        saw_modules_key = true;
        let Expression::ArrayExpression(arr) = &p.value else {
            return Err(ExtractError::ComponentBadModules {
                kind,
                class: class_name.to_owned(),
                span: p.span,
            });
        };
        for elem in &arr.elements {
            let oxc_ast::ast::ArrayExpressionElement::Identifier(id) = elem else {
                return Err(ExtractError::ComponentBadModules {
                    kind,
                    class: class_name.to_owned(),
                    span: arr.span,
                });
            };
            let name = id.name.as_str();
            let key = resolve_name_to_key(name, imports, local_classes);
            // `resolve_name_to_key` always yields a `Key::Class`; module
            // identifiers are class identifiers by construction. Anything
            // else here would be a parser bug.
            match key {
                Key::Class {
                    module,
                    name: exported,
                    ..
                } => modules.push(ClassRef {
                    module,
                    name: exported,
                }),
                Key::Set { .. } | Key::Token { .. } => {
                    return Err(ExtractError::ComponentBadModules {
                        kind,
                        class: class_name.to_owned(),
                        span: arr.span,
                    });
                }
            }
        }
    }
    let _ = saw_modules_key; // missing `modules:` is allowed (treated as empty)
    Ok(modules)
}
