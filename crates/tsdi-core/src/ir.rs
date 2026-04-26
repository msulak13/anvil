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
}

/// A module path. M1 stores the raw import specifier; M2 normalizes to an
/// absolute filesystem path.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ModulePath(pub String);

impl ModulePath {
    /// Sentinel used by the parser when a referenced type is declared in
    /// the same file as the reference. M2's resolver replaces this with the
    /// file's own absolute path during cross-file pass.
    pub const SAME_FILE: &'static str = "<self>";

    /// Construct a [`ModulePath`] for a same-file reference.
    #[must_use]
    pub fn same_file() -> Self {
        Self(Self::SAME_FILE.to_owned())
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
    ProvidesMethod {
        /// The owning module class.
        module: ClassRef,
        /// The method name on the module.
        method: String,
    },
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
#[derive(Clone, Debug)]
pub struct EntryPoint {
    /// The method name as written in the abstract component class.
    pub name: String,
    /// The key that this entry point exposes.
    pub key: Key,
    /// Where the abstract method appears in source.
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
            module: ModulePath("/abs/heater.ts".into()),
            name: "Heater".into(),
        };
        let b = Key::Class {
            module: ModulePath("/abs/heater.ts".into()),
            name: "Heater".into(),
        };
        let c = Key::Class {
            module: ModulePath("/abs/other.ts".into()),
            name: "Heater".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
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
        assert_eq!(ModulePath::same_file().0, "<self>");
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
