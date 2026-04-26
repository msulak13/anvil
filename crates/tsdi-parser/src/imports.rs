//! Single-file import map.
//!
//! Maps a local identifier (the name visible inside the file) to the
//! `(specifier, exported_name)` pair the file imported it from. The result
//! is consumed by the decorator extractor when minting `Key` values for
//! type references.
//!
//! # Scope of M1
//!
//! - **Named imports** are supported, including `as` rename:
//!   `import { Heater } from "./heater"` — local `Heater` → (`./heater`, `Heater`)
//!   `import { Heater as H } from "./heater"` — local `H` → (`./heater`, `Heater`)
//! - **Default imports** are supported; the exported name in our map is
//!   `default` (callers should treat that as opaque):
//!   `import Pump from "./pump"` — local `Pump` → (`./pump`, `default`)
//! - **Type-only imports** (`import type { … }`) are recognized identically
//!   to value imports, since type identifiers are exactly what we resolve.
//! - **Namespace imports** (`import * as M from …`) are intentionally
//!   *not* supported in M1; member expressions `M.Foo` are out of scope.
//!   The parser emits a clear diagnostic when it encounters one used as a
//!   decorator target or type annotation. Lifted in M2.

use std::collections::HashMap;

use oxc_ast::ast::{ImportDeclarationSpecifier, Statement};

/// One entry in the import map: where a local identifier came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportSource {
    /// Module specifier as written in the `from` clause.
    pub specifier: String,
    /// Exported name on the source side. `"default"` for default imports.
    pub exported_name: String,
}

/// Map from local identifier name → its origin import.
///
/// Class declarations and other in-file declarations are *not* present in
/// this map; the decorator extractor handles same-file references via
/// `ModulePath::SAME_FILE` (see `tsdi_core::ir`).
pub type ImportMap = HashMap<String, ImportSource>;

/// Build an [`ImportMap`] from a parsed program.
///
/// Iterates the top-level `Statement::ModuleDeclaration` import variants and
/// flattens their specifiers into the local-name → origin map. Unknown
/// specifier shapes (namespace imports, side-effect-only imports) are
/// silently skipped — the decorator extractor will report a missing-import
/// diagnostic later if a referenced identifier turns out not to be in the
/// map.
#[must_use]
pub fn build_import_map(program: &oxc_ast::ast::Program<'_>) -> ImportMap {
    let mut map: ImportMap = HashMap::new();
    for stmt in &program.body {
        let Statement::ImportDeclaration(decl) = stmt else {
            continue;
        };
        // Skip side-effect-only imports (`import "./register"`).
        let Some(specifiers) = decl.specifiers.as_ref() else {
            continue;
        };
        let module_specifier = decl.source.value.as_str().to_owned();
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let exported_name = s.imported.name().as_str().to_owned();
                    let local_name = s.local.name.as_str().to_owned();
                    map.insert(
                        local_name,
                        ImportSource {
                            specifier: module_specifier.clone(),
                            exported_name,
                        },
                    );
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    map.insert(
                        s.local.name.as_str().to_owned(),
                        ImportSource {
                            specifier: module_specifier.clone(),
                            exported_name: "default".to_owned(),
                        },
                    );
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => {
                    // M1 limitation; see module docs.
                }
            }
        }
    }
    map
}

/// Names declared at the top level of the file (classes, functions, vars).
///
/// Used by the decorator extractor to recognize same-file references that
/// won't appear in the import map. We only need class names for v0.1 but
/// keep the door open for `@Provides` on top-level functions later.
pub fn collect_local_class_names(program: &oxc_ast::ast::Program<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for stmt in &program.body {
        let class = match stmt {
            Statement::ClassDeclaration(c) => Some(c),
            Statement::ExportNamedDeclaration(decl) => match &decl.declaration {
                Some(oxc_ast::ast::Declaration::ClassDeclaration(c)) => Some(c),
                _ => None,
            },
            Statement::ExportDefaultDeclaration(_) => {
                // Default exports of class declarations are uncommon for DI
                // and not exercised by v0.1 fixtures; revisit if needed.
                None
            }
            _ => None,
        };
        if let Some(class) = class {
            if let Some(id) = &class.id {
                names.push(id.name.as_str().to_owned());
            }
        }
    }
    names
}
