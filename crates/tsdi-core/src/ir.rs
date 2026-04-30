//! Intermediate representation for the binding graph.
//!
//! Filled in by `tsdi-parser`, consumed by `tsdi-codegen`. See `docs/ir.md`
//! for the full specification and a worked example mapping user TS to IR.

/// Stable identity for a TypeScript type without invoking a full type checker.
///
/// We approximate type identity by `(module path, exported name)` pairs
/// resolved through the file's import map.
///
/// In **M1** the `module` field carries the **import specifier verbatim**
/// (e.g. `"./heater"`, `"tsdi"`, `"my-pkg/sub"`). M2 introduces a resolver
/// that rewrites these to absolute, normalized filesystem paths so that
/// equivalent imports across files compare equal.
///
/// For type identifiers declared in the same file as their reference (i.e.
/// not in the import map), the parser uses [`ModulePath::SAME_FILE`] as a
/// sentinel; M2 resolves these against the file's actual path.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Key {
    /// An imported (or locally declared) class, identified by its module
    /// and exported name.
    Class {
        /// Module specifier (M1) or absolute path (M2+).
        module: ModulePath,
        /// The exported name as it appears at the declaration site.
        name: String,
    },
    /// A `Set<T>` multibinding aggregate (M9). Multiple `@IntoSet`
    /// contributions whose return type is the element type collapse into a
    /// single binding under this key.
    Set {
        /// The element type being collected. Always a [`Key::Class`] in v0.1.
        element: Box<Key>,
    },
}

/// A module path. M1 stores the raw import specifier in `abs`; M2's
/// resolver rewrites `abs` to an absolute filesystem path while keeping
/// the user's original specifier in `original` so `tsdi-codegen` can
/// re-emit the same import shape (essential for `node_modules` packages
/// where a relative path doesn't make sense — `import { Request } from
/// "express"` instead of `import { Request } from "../../node_modules/..."`).
///
/// **Equality is on `abs` only.** Two `ModulePath` values compare equal
/// iff they refer to the same file, regardless of how each importer
/// happened to spell the specifier. This preserves M2's contract that
/// equivalent imports across files produce equal `Key`s.
///
/// The struct is **not** a tuple anymore. Construction goes through the
/// named-field syntax or the convenience constructors below.
#[derive(Clone, Debug)]
pub struct ModulePath {
    /// Absolute, canonical filesystem path (M2+) or the [`Self::SAME_FILE`]
    /// sentinel (M1, pre-resolution).
    pub abs: String,
    /// The user's original import specifier (e.g. `"./heater"`, `"express"`).
    /// `None` for same-file references and for tooling-built paths
    /// (graph tests, golden fixtures) that have no source specifier.
    pub original: Option<String>,
}

impl ModulePath {
    /// Sentinel used by the parser when a referenced type is declared in
    /// the same file as the reference. M2's resolver replaces this with the
    /// file's own absolute path during the cross-file pass.
    pub const SAME_FILE: &'static str = "<self>";

    /// Construct a [`ModulePath`] for a same-file reference. `original` is
    /// [`None`] because there is no user-written specifier — the type is
    /// declared in the importing file itself.
    #[must_use]
    pub fn same_file() -> Self {
        Self {
            abs: Self::SAME_FILE.to_owned(),
            original: None,
        }
    }

    /// Build from a user-written import specifier. Initially both `abs`
    /// and `original` carry the specifier verbatim; M2's resolver then
    /// rewrites `abs` to the canonical absolute path. `original` is
    /// preserved through that pass so codegen can prefer it for
    /// `node_modules` imports.
    #[must_use]
    pub fn from_specifier(spec: impl Into<String>) -> Self {
        let s = spec.into();
        Self {
            abs: s.clone(),
            original: Some(s),
        }
    }

    /// Build from a known-absolute path with no preserved specifier.
    /// Used by graph tests and tooling that doesn't need to round-trip
    /// import emission. Codegen falls back to a relative-path
    /// computation when `original` is `None`.
    #[must_use]
    pub fn from_abs(abs: impl Into<String>) -> Self {
        Self {
            abs: abs.into(),
            original: None,
        }
    }

    /// Whether `abs` resolves into a `node_modules` directory. Codegen
    /// uses this to decide whether to emit the user's bare specifier
    /// (e.g. `"express"`) instead of a brittle relative path.
    #[must_use]
    pub fn is_node_modules(&self) -> bool {
        self.abs.contains("node_modules")
    }
}

impl PartialEq for ModulePath {
    fn eq(&self, other: &Self) -> bool {
        self.abs == other.abs
    }
}
impl Eq for ModulePath {}
impl std::hash::Hash for ModulePath {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.abs.hash(state);
    }
}

/// A byte-offset range into a source file, plus the file's path.
///
/// Parser-agnostic: tsdi-core stays free of any Oxc dependency. The
/// parser converts `oxc_span::Span` into `SourceSpan` at extraction time
/// (M1+); M3 validation propagates these into diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    /// Source file path. M2 onward this is absolute and canonical.
    pub path: String,
    /// Inclusive byte offset of the first character.
    pub start: u32,
    /// Exclusive byte offset just past the last character.
    pub end: u32,
}

impl SourceSpan {
    /// Construct a source span. Convenience for parsers and tests.
    #[must_use]
    pub fn new(path: impl Into<String>, start: u32, end: u32) -> Self {
        Self {
            path: path.into(),
            start,
            end,
        }
    }

    /// Length of the span in bytes.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span has zero length.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// The lifetime/scope of a [`Binding`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    /// A new instance per request.
    Unscoped,
    /// One instance per owning component.
    Singleton,
}

/// A reference to a class declaration in source.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClassRef {
    /// Module specifier (M1) or absolute path (M2+) of the file containing
    /// the class.
    pub module: ModulePath,
    /// The class's exported name.
    pub name: String,
}

/// How an instance for a [`Key`] is produced.
#[derive(Clone, Debug)]
pub enum Provider {
    /// Constructor injection on a class annotated with `@Inject`.
    InjectCtor {
        /// The class whose constructor is invoked.
        class: ClassRef,
    },
    /// A static `@Provides` method on a `@Module` class.
    ///
    /// `is_async` (M12) is set when the method is declared `async`
    /// (its return type is `Promise<T>`). The codegen treats it
    /// specially: instead of a sync getter, the value is awaited during
    /// the dagger's resolution phase and the **resolved** value is
    /// stashed in the cache field. Downstream consumers — entry points,
    /// `@Inject` ctors, subcomponents — see only the resolved type.
    ProvidesMethod {
        /// The owning module class.
        module: ClassRef,
        /// The method name on the module.
        method: String,
        /// Whether the method was declared `async`. M12.
        is_async: bool,
    },
    /// An abstract `@Binds` method on a `@Module` class. The binding's
    /// `key` is the method's return type; this provider redirects all
    /// requests for that key to the [`Binding`] for `target`.
    ///
    /// At codegen time the factory body is just `return this.<getTarget>()`,
    /// inheriting whatever scope the target binding has.
    Binds {
        /// The implementation key this binding aliases.
        target: Key,
    },
    /// A synthesized aggregate of `@IntoSet` contributions (M9).
    ///
    /// Produced exclusively by the graph aggregator — parsers never emit
    /// this provider directly. The binding's [`Key`] is `Key::Set { element }`
    /// and its factory is `new Set<element>([contributors...])`.
    SetMultibinding {
        /// One [`SetContributor`] per `@IntoSet @Provides` method that
        /// targets this set's element type. Order is the source order of
        /// the contributing modules.
        contributors: Vec<SetContributor>,
    },
    /// A virtual binding whose value is supplied by the **caller** of a
    /// subcomponent factory rather than constructed by the dagger (M11).
    ///
    /// Produced exclusively by the graph layer when a subcomponent's
    /// parent factory has formal parameters — each parameter becomes a
    /// `FactoryParam` binding visible only inside the child graph. The
    /// codegen emits a private field on the child dagger that stores the
    /// runtime-supplied value, plus a trivial getter `private getX(): T
    /// { return this.<param>; }` so call sites stay uniform with every
    /// other binding's `getX()` shape.
    FactoryParam {
        /// The constructor field / getter name on the child dagger
        /// (taken verbatim from the parent factory's parameter name).
        name: String,
    },
}

/// One `@IntoSet @Provides` contribution to a [`Provider::SetMultibinding`].
#[derive(Clone, Debug)]
pub struct SetContributor {
    /// The owning `@Module` class (the contribution is always a `static`
    /// method on a `@Module` for v0.1).
    pub module: ClassRef,
    /// The static method name on the module.
    pub method: String,
    /// Keys for the contributor method's parameters.
    pub deps: Vec<Key>,
    /// Where the contribution method appears in source.
    pub source: SourceSpan,
}

/// Whether a binding participates in a multibinding aggregation.
///
/// Set on raw bindings produced by the parser. The graph aggregator folds
/// every binding with a non-`None` role into a single synthesized binding
/// (with provider [`Provider::SetMultibinding`]) whose own role is `None`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MultibindRole {
    /// Regular (non-multibinding) binding.
    #[default]
    None,
    /// Contribution to a `Set<T>` multibinding (M9).
    IntoSet,
}

/// A single binding in the dependency graph.
#[derive(Clone, Debug)]
pub struct Binding {
    /// The key this binding satisfies.
    pub key: Key,
    /// How an instance is produced.
    pub provider: Provider,
    /// The scope at which instances are cached.
    pub scope: Scope,
    /// The keys this binding requires to construct an instance.
    pub deps: Vec<Key>,
    /// Where this binding appears in source (used for diagnostics).
    pub source: SourceSpan,
    /// Whether this binding is a multibinding contribution (M9). Always
    /// [`MultibindRole::None`] on the synthesized aggregate produced by the
    /// graph layer; [`MultibindRole::IntoSet`] on raw `@IntoSet`
    /// contributions emitted by the parser.
    pub role: MultibindRole,
}

/// A `@Module` declaration: a class hosting `@Provides` factory methods.
#[derive(Clone, Debug)]
pub struct ModuleDecl {
    /// The module class itself.
    pub class: ClassRef,
    /// All bindings contributed by `@Provides` methods on this module.
    pub provides: Vec<Binding>,
    /// Where the `@Module` class appears in source.
    pub source: SourceSpan,
}

/// An entry point on a `@Component` — an abstract method whose return type
/// is a [`Key`].
///
/// For zero-arg methods (the v0.1 baseline) `factory_params` is empty and
/// the method is a regular getter on the dagger. For `@Subcomponent`
/// factories (M11), each parameter on the parent's abstract method
/// becomes a [`FactoryParam`] threaded into the child dagger's
/// constructor. The graph layer materializes those as
/// [`Provider::FactoryParam`] bindings inside the child's binding map.
#[derive(Clone, Debug)]
pub struct EntryPoint {
    /// The method name as written in the abstract component class.
    pub name: String,
    /// The key that this entry point exposes.
    pub key: Key,
    /// Where the abstract method appears in source.
    pub source: SourceSpan,
    /// Formal parameters on the entry-point method (M11). Empty for
    /// regular `@Component` entry points; populated for `@Subcomponent`
    /// factory methods that thread runtime-supplied state (e.g.
    /// `req: Request`) into the child graph.
    pub factory_params: Vec<FactoryParam>,
}

/// A formal parameter on an entry-point method that becomes a virtual
/// binding inside the child graph (M11).
///
/// Captured by the parser from the parameter's identifier and type
/// annotation. The graph layer rewrites each `FactoryParam` into a
/// [`Binding`] with [`Provider::FactoryParam`]; the codegen materializes
/// that as a constructor argument plus a private field on the child
/// dagger.
#[derive(Clone, Debug)]
pub struct FactoryParam {
    /// The parameter identifier (used as the field name on the dagger).
    pub name: String,
    /// The parameter's declared type, lowered to a [`Key`].
    pub key: Key,
    /// Where the parameter appears in source.
    pub source: SourceSpan,
}

/// A `@Component` declaration: the root of an object graph.
#[derive(Clone, Debug)]
pub struct ComponentDecl {
    /// The abstract component class.
    pub class: ClassRef,
    /// The modules included by this component (their class refs in source order).
    pub modules: Vec<ClassRef>,
    /// The component's own scope (typically `Singleton` if `@Singleton`-annotated).
    pub scope: Scope,
    /// Abstract methods exposing the graph to user code.
    pub entry_points: Vec<EntryPoint>,
    /// Where the `@Component` class appears in source.
    pub source: SourceSpan,
}

/// A `@Subcomponent` declaration: a child object graph that inherits its
/// parent component's bindings and adds its own modules on top.
///
/// Structurally identical to [`ComponentDecl`] — separating the type lets
/// the codegen and validator dispatch on whether an abstract method is a
/// regular entry point or a subcomponent factory. A subcomponent's
/// `entry_points` are exposed via the generated `Dagger<Sub>` class; the
/// parent dagger is held as a constructor-injected back-reference so
/// inherited bindings route through the parent's factories.
#[derive(Clone, Debug)]
pub struct SubcomponentDecl {
    /// The abstract subcomponent class.
    pub class: ClassRef,
    /// The modules included by this subcomponent (their class refs in source order).
    pub modules: Vec<ClassRef>,
    /// The subcomponent's own scope.
    pub scope: Scope,
    /// Abstract methods exposing this subcomponent's graph.
    pub entry_points: Vec<EntryPoint>,
    /// Where the `@Subcomponent` class appears in source.
    pub source: SourceSpan,
}

/// Everything a single `.ts` file contributes to the IR.
///
/// Produced by `tsdi-parser::parse_file`. Aggregated across files by the
/// CLI before being handed to `tsdi-core`'s graph builder.
#[derive(Clone, Debug, Default)]
pub struct ParsedFile {
    /// Source path of this file (informational; carried through to diagnostics).
    pub path: String,
    /// `@Module` classes declared in this file.
    pub modules: Vec<ModuleDecl>,
    /// `@Component` classes declared in this file.
    pub components: Vec<ComponentDecl>,
    /// `@Subcomponent` classes declared in this file.
    pub subcomponents: Vec<SubcomponentDecl>,
    /// Self-bindings produced by classes whose constructor is annotated
    /// `@Inject`. The binding's `Key` is the class itself; the provider is
    /// always [`Provider::InjectCtor`].
    pub inject_classes: Vec<Binding>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_equality_uses_module_and_name() {
        let a = Key::Class {
            module: ModulePath::from_abs("/abs/heater.ts"),
            name: "Heater".into(),
        };
        let b = Key::Class {
            module: ModulePath::from_abs("/abs/heater.ts"),
            name: "Heater".into(),
        };
        let c = Key::Class {
            module: ModulePath::from_abs("/abs/other.ts"),
            name: "Heater".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn module_path_equality_ignores_original_specifier() {
        // Two importers may spell the same file differently — equality
        // must be on `abs` only or M2's resolver contract breaks.
        let from_relative = ModulePath {
            abs: "/abs/heater.ts".into(),
            original: Some("./heater".into()),
        };
        let from_alias = ModulePath {
            abs: "/abs/heater.ts".into(),
            original: Some("@/heater".into()),
        };
        assert_eq!(from_relative, from_alias);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut hasher2 = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&from_relative, &mut hasher);
        std::hash::Hash::hash(&from_alias, &mut hasher2);
        assert_eq!(
            std::hash::Hasher::finish(&hasher),
            std::hash::Hasher::finish(&hasher2),
            "Hash must agree with PartialEq",
        );
    }

    #[test]
    fn from_specifier_seeds_both_fields() {
        let mp = ModulePath::from_specifier("./pump");
        assert_eq!(mp.abs, "./pump");
        assert_eq!(mp.original.as_deref(), Some("./pump"));
    }

    #[test]
    fn is_node_modules_detection() {
        let local = ModulePath::from_abs("/abs/proj/src/heater.ts");
        let pkg = ModulePath::from_abs("/abs/proj/node_modules/express/index.d.ts");
        assert!(!local.is_node_modules());
        assert!(pkg.is_node_modules());
    }

    #[test]
    fn scope_is_copy() {
        // Compile-time assertion: this fn would not compile if Scope weren't Copy.
        fn require_copy<T: Copy>() {}
        require_copy::<Scope>();
        assert_eq!(Scope::Singleton, Scope::Singleton);
    }

    #[test]
    fn same_file_sentinel_is_stable() {
        let mp = ModulePath::same_file();
        assert_eq!(mp.abs, "<self>");
        assert!(mp.original.is_none());
    }

    #[test]
    fn source_span_len_and_empty() {
        let s = SourceSpan::new("/x.ts", 10, 20);
        assert_eq!(s.len(), 10);
        assert!(!s.is_empty());
        let e = SourceSpan::new("/x.ts", 5, 5);
        assert!(e.is_empty());
        assert_eq!(e.len(), 0);
    }
}
