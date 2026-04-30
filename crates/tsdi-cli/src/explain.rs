//! Implementation of `tsdi explain`.
//!
//! Given a key name (e.g. `Pump`), walks the project's binding catalog
//! and prints the dep tree rooted at the first binding whose key name
//! matches. Output is plain text — no diagnostics — so it can be piped
//! into other tools.
//!
//! # Output shape
//!
//! ```text
//! Pump@/abs/pump.ts (InjectCtor, Unscoped)
//! └─ Heater@/abs/heater.ts (InjectCtor, Singleton)
//! ```

use std::collections::HashMap;

use tsdi_core::ir::{Binding, Key, Provider, Scope};

use crate::{CheckError, ProjectIr};

/// Render the dep tree for `key_name` to stdout.
///
/// # Errors
/// Returns a `CheckError` if no binding matches `key_name`.
pub(crate) fn run(key_name: &str, ir: &ProjectIr) -> Result<(), CheckError> {
    let bindings = collect_bindings(ir);

    // Find every binding whose key name matches; lex-sort by module so
    // ties are stable.
    let mut hits: Vec<&Binding> = bindings
        .values()
        .filter(|b| key_name_of(&b.key) == key_name)
        .collect();
    hits.sort_by_key(|b| key_module(&b.key).to_owned());

    let Some(root) = hits.first() else {
        return Err(CheckError::Other(anyhow::anyhow!(
            "no binding named `{key_name}` in the project"
        )));
    };
    if hits.len() > 1 {
        eprintln!(
            "warning: {} bindings named `{key_name}`; explaining the first ({}). Pass a fuller path to disambiguate (deferred to v0.2).",
            hits.len(),
            key_module(&root.key),
        );
    }

    print_binding(root, &bindings, "", true);
    Ok(())
}

fn collect_bindings(ir: &ProjectIr) -> HashMap<Key, Binding> {
    let mut out: HashMap<Key, Binding> = HashMap::new();
    for b in &ir.inject_classes {
        out.insert(b.key.clone(), b.clone());
    }
    for m in &ir.modules {
        for b in &m.provides {
            out.insert(b.key.clone(), b.clone());
        }
    }
    out
}

fn key_name_of(k: &Key) -> String {
    match k {
        Key::Class { name, .. } => name.clone(),
        Key::Set { element } => format!("Set<{}>", key_name_of(element)),
    }
}

fn key_module(k: &Key) -> &str {
    match k {
        Key::Class { module, .. } => module.abs.as_str(),
        Key::Set { element } => key_module(element),
    }
}

fn provider_label(p: &Provider) -> &'static str {
    match p {
        Provider::InjectCtor { .. } => "InjectCtor",
        Provider::ProvidesMethod { .. } => "ProvidesMethod",
        Provider::Binds { .. } => "Binds",
        Provider::SetMultibinding { .. } => "SetMultibinding",
        Provider::FactoryParam { .. } => "FactoryParam",
    }
}

fn scope_label(s: Scope) -> &'static str {
    match s {
        Scope::Unscoped => "Unscoped",
        Scope::Singleton => "Singleton",
    }
}

fn print_binding(b: &Binding, table: &HashMap<Key, Binding>, prefix: &str, is_root: bool) {
    if is_root {
        println!(
            "{}@{} ({}, {})",
            key_name_of(&b.key),
            key_module(&b.key),
            provider_label(&b.provider),
            scope_label(b.scope),
        );
    }
    let last = b.deps.len().saturating_sub(1);
    for (i, dep) in b.deps.iter().enumerate() {
        let is_last = i == last;
        let connector = if is_last { "└─ " } else { "├─ " };
        let next_prefix = if is_last {
            format!("{prefix}   ")
        } else {
            format!("{prefix}│  ")
        };
        match table.get(dep) {
            Some(child) => {
                println!(
                    "{}{}{}@{} ({}, {})",
                    prefix,
                    connector,
                    key_name_of(&child.key),
                    key_module(&child.key),
                    provider_label(&child.provider),
                    scope_label(child.scope),
                );
                print_binding(child, table, &next_prefix, false);
            }
            None => {
                println!(
                    "{}{}{}@{} (UNRESOLVED)",
                    prefix,
                    connector,
                    key_name_of(dep),
                    key_module(dep),
                );
            }
        }
    }
}
