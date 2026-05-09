//! Validation rules over the dependency graph.
//!
//! M3 ships the four rules called out in `docs/validation.md`:
//!
//! 1. [`DiagnosticKind::MissingBinding`] — a `Key` was requested but no
//!    provider exists for it.
//! 2. [`DiagnosticKind::Cycle`] — the graph has a strongly-connected
//!    component of size ≥ 2 (or a self-loop).
//! 3. [`DiagnosticKind::Duplicate`] — two providers exist for the same key
//!    in the same component.
//! 4. [`DiagnosticKind::ScopeMismatch`] — a `Singleton` binding sits inside
//!    an `Unscoped` component.
//!
//! The validator produces a [`Vec<Diagnostic>`] of structured records.
//! Rendering them as `miette` reports is the CLI's job — `anvil-core` stays
//! free of I/O so it can be tested without touching disk.
//!
//! The detection logic lives next to the graph builder in
//! [`crate::graph::build_and_validate`]; this module owns the data types
//! the builder produces.

use thiserror::Error;

use crate::ir::{ClassRef, Key, Scope, SourceSpan};

/// A single problem found during graph construction or validation.
///
/// A `Diagnostic` is pure data: it carries spans pointing into source files
/// but not the source contents themselves. The CLI loads file contents and
/// dresses these into [`miette::Report`]s for display.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// What kind of problem this is.
    pub kind: DiagnosticKind,
    /// The primary location the diagnostic should anchor on.
    pub primary: Label,
    /// Additional related locations (e.g. the second of two duplicate
    /// declarations, or the requesting binding for a missing dep).
    pub related: Vec<Label>,
}

impl Diagnostic {
    /// Stable diagnostic code suitable for use in `miette` outputs.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self.kind {
            DiagnosticKind::MissingBinding { .. } => "anvil::missing_binding",
            DiagnosticKind::Cycle { .. } => "anvil::cycle",
            DiagnosticKind::Duplicate { .. } => "anvil::duplicate",
            DiagnosticKind::ScopeMismatch { .. } => "anvil::scope_mismatch",
            DiagnosticKind::FactoryParamsOnNonSubcomponentEntry { .. } => {
                "anvil::factory_params_on_non_subcomponent"
            }
            DiagnosticKind::DuplicateFactoryParam { .. } => "anvil::duplicate_factory_param",
            DiagnosticKind::SingletonSubcomponentWithFactoryParams { .. } => {
                "anvil::singleton_subcomponent_with_factory_params"
            }
            DiagnosticKind::AsyncBindingNeedsSingletonComponent { .. } => {
                "anvil::async_needs_singleton"
            }
        }
    }

    /// One-line human-readable summary.
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.kind {
            DiagnosticKind::MissingBinding { key, requested_by } => match requested_by {
                Some(rb) => format!(
                    "missing binding for {} (requested by {})",
                    key_display(key),
                    key_display(rb)
                ),
                None => format!("missing binding for {}", key_display(key)),
            },
            DiagnosticKind::Cycle { keys } => {
                let parts: Vec<String> = keys.iter().map(key_display).collect();
                format!("dependency cycle: {}", parts.join(" -> "))
            }
            DiagnosticKind::Duplicate { key } => {
                format!("duplicate binding for {}", key_display(key))
            }
            DiagnosticKind::ScopeMismatch {
                key,
                binding_scope,
                component_scope,
            } => format!(
                "scope mismatch on {}: binding is {:?} but component is {:?}",
                key_display(key),
                binding_scope,
                component_scope,
            ),
            DiagnosticKind::FactoryParamsOnNonSubcomponentEntry { component, method } => format!(
                "@Component entry point `{}.{}` cannot take factory parameters",
                component.name, method,
            ),
            DiagnosticKind::DuplicateFactoryParam { key } => format!(
                "duplicate factory parameter type {} on a single subcomponent factory",
                key_display(key),
            ),
            DiagnosticKind::SingletonSubcomponentWithFactoryParams { subcomponent } => format!(
                "@Singleton @Subcomponent `{}` cannot take factory parameters",
                subcomponent.name,
            ),
            DiagnosticKind::AsyncBindingNeedsSingletonComponent { key, component } => format!(
                "async @Provides binding for {} in non-@Singleton component `{}` — cache it with @Singleton or move it into a @Subcomponent",
                key_display(key),
                component.name,
            ),
        }
    }
}

/// A source-anchored sub-message attached to a [`Diagnostic`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Label {
    /// Location this label points at.
    pub span: SourceSpan,
    /// Human-readable note rendered next to the source span.
    pub message: String,
}

/// Discriminator for [`Diagnostic`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    /// A binding was requested for which no provider exists.
    MissingBinding {
        /// The unsatisfied key.
        key: Key,
        /// The key whose construction triggered the lookup, if any.
        /// `None` for an entry-point that has no binding.
        requested_by: Option<Key>,
    },
    /// A cycle exists in the dependency graph.
    Cycle {
        /// The keys participating in the cycle, in traversal order.
        keys: Vec<Key>,
    },
    /// More than one binding declared for the same key in the same component.
    Duplicate {
        /// The key with multiple declarations.
        key: Key,
    },
    /// A `Singleton` binding inside a non-`Singleton` component.
    ScopeMismatch {
        /// The offending key.
        key: Key,
        /// The binding's declared scope.
        binding_scope: Scope,
        /// The enclosing component's scope.
        component_scope: Scope,
    },
    /// A regular `@Component` entry-point method declared with formal
    /// parameters (M11). `createX()` is zero-arg, so the params would
    /// have nowhere to come from. Factory parameters are only valid on
    /// abstract methods that resolve to `@Subcomponent` factories.
    FactoryParamsOnNonSubcomponentEntry {
        /// The owning component class.
        component: ClassRef,
        /// The offending entry-point method name.
        method: String,
    },
    /// Two factory parameters on the same subcomponent factory share a
    /// type — child bindings asking for that type would be ambiguous.
    DuplicateFactoryParam {
        /// The collided key.
        key: Key,
    },
    /// `@Singleton` was applied to a `@Subcomponent` whose parent
    /// factory takes runtime parameters. The cache would freeze the
    /// first call's argument values across every subsequent call —
    /// almost certainly a bug.
    SingletonSubcomponentWithFactoryParams {
        /// The offending subcomponent class.
        subcomponent: ClassRef,
    },
    /// An async `@Provides` binding sits inside a non-`@Singleton`
    /// component (M12). Without `@Singleton` the dagger has no place to
    /// cache the resolved value — every call to a sync entry point
    /// would need to await again, which would force the entry point
    /// itself to be async. Subcomponents are exempt: they're already
    /// fresh per-call, so per-call awaits are fine there.
    AsyncBindingNeedsSingletonComponent {
        /// The async binding's key.
        key: Key,
        /// The component class hosting the binding.
        component: ClassRef,
    },
}

/// Compact display form for a [`Key`] used inside diagnostic messages.
#[must_use]
pub fn key_display(key: &Key) -> String {
    match key {
        Key::Class { module, name } => format!("{name}@{}", module.abs),
        Key::Set { element } => format!("Set<{}>", key_display(element)),
    }
}

/// Wrapper that lets a [`Diagnostic`] participate in the `Result`/`?` flow
/// for callers that prefer error-propagation style over diagnostic vectors.
#[derive(Debug, Error)]
#[error("{summary}")]
pub struct ValidationError {
    /// All diagnostics produced for this validation run.
    pub diagnostics: Vec<Diagnostic>,
    /// Pre-rendered first-line summary, used by `Display`/`Error`.
    summary: String,
}

impl ValidationError {
    /// Build an error from a non-empty diagnostic list.
    ///
    /// Returns `None` if `diagnostics` is empty (no problem to report).
    #[must_use]
    pub fn from_diagnostics(diagnostics: Vec<Diagnostic>) -> Option<Self> {
        if diagnostics.is_empty() {
            return None;
        }
        let summary = if diagnostics.len() == 1 {
            diagnostics[0].summary()
        } else {
            format!(
                "{} validation errors (first: {})",
                diagnostics.len(),
                diagnostics[0].summary()
            )
        };
        Some(Self {
            diagnostics,
            summary,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Key, ModulePath};

    fn k(name: &str) -> Key {
        Key::Class {
            module: ModulePath::from_abs(format!("/proj/{name}.ts")),
            name: name.to_owned(),
        }
    }

    #[test]
    fn key_display_roundtrips() {
        assert_eq!(key_display(&k("Heater")), "Heater@/proj/Heater.ts");
    }

    #[test]
    fn missing_binding_summary() {
        let d = Diagnostic {
            kind: DiagnosticKind::MissingBinding {
                key: k("Heater"),
                requested_by: Some(k("Pump")),
            },
            primary: Label {
                span: SourceSpan::new("/x.ts", 0, 0),
                message: "requested here".into(),
            },
            related: vec![],
        };
        assert_eq!(
            d.summary(),
            "missing binding for Heater@/proj/Heater.ts (requested by Pump@/proj/Pump.ts)"
        );
        assert_eq!(d.code(), "anvil::missing_binding");
    }

    #[test]
    fn cycle_summary() {
        let d = Diagnostic {
            kind: DiagnosticKind::Cycle {
                keys: vec![k("A"), k("B"), k("A")],
            },
            primary: Label {
                span: SourceSpan::new("/x.ts", 0, 0),
                message: "in cycle".into(),
            },
            related: vec![],
        };
        assert!(d.summary().starts_with("dependency cycle: "));
        assert_eq!(d.code(), "anvil::cycle");
    }

    #[test]
    fn validation_error_from_empty_is_none() {
        assert!(ValidationError::from_diagnostics(vec![]).is_none());
    }

    #[test]
    fn validation_error_from_one() {
        let d = Diagnostic {
            kind: DiagnosticKind::Duplicate { key: k("X") },
            primary: Label {
                span: SourceSpan::new("/x.ts", 0, 0),
                message: "first".into(),
            },
            related: vec![],
        };
        let err = ValidationError::from_diagnostics(vec![d]).unwrap();
        assert_eq!(err.diagnostics.len(), 1);
        assert!(err.to_string().contains("duplicate binding"));
    }
}
