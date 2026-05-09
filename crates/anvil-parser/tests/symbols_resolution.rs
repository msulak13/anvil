//! Integration tests for the M2 cross-file resolver.
//!
//! Each test materializes a small TypeScript project on disk in a tempdir,
//! runs `ProjectGraph::build_from_entry`, and asserts that every
//! `Key::Class` in the produced IR carries an absolute, canonical path —
//! the M2 acceptance criterion.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use anvil_core::ir::{Key, ModulePath, ParsedFile};
use anvil_parser::symbols::{ProjectGraph, ProjectResolver};

/// Materialize a multi-file project under `tmp` and return its root.
fn write_project(tmp: &TempDir, files: &[(&str, &str)]) -> PathBuf {
    let root = tmp.path().to_path_buf();
    for (rel, contents) in files {
        let dst = root.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&dst, contents).unwrap();
    }
    root
}

/// Walk every Key in `parsed` and call `f` on its module path.
fn for_each_key<'a>(parsed: &'a ParsedFile, mut f: impl FnMut(&'a ModulePath)) {
    fn visit_key<'a>(k: &'a Key, f: &mut impl FnMut(&'a ModulePath)) {
        match k {
            Key::Class { module, .. } => f(module),
            Key::Set { element } => visit_key(element, f),
        }
    }
    for m in &parsed.modules {
        f(&m.class.module);
        for b in &m.provides {
            visit_key(&b.key, &mut f);
            for d in &b.deps {
                visit_key(d, &mut f);
            }
        }
    }
    for c in &parsed.components {
        f(&c.class.module);
        for cm in &c.modules {
            f(&cm.module);
        }
        for ep in &c.entry_points {
            visit_key(&ep.key, &mut f);
        }
    }
    for b in &parsed.inject_classes {
        visit_key(&b.key, &mut f);
        for d in &b.deps {
            visit_key(d, &mut f);
        }
    }
}

fn assert_all_keys_absolute(parsed: &ParsedFile) {
    for_each_key(parsed, |mp| {
        assert!(
            mp.abs != ModulePath::SAME_FILE,
            "found unrewritten SAME_FILE sentinel in {:?}",
            parsed.path,
        );
        assert!(
            Path::new(&mp.abs).is_absolute(),
            "expected absolute module path, got {:?} in {:?}",
            mp.abs,
            parsed.path,
        );
    });
}

#[test]
fn relative_imports_are_resolved_to_absolute_paths() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            (
                "src/heater.ts",
                r#"
                    import { Inject, Singleton } from "@msulak/anvil";
                    @Inject
                    @Singleton
                    export class Heater {
                        constructor() {}
                    }
                "#,
            ),
            (
                "src/pump.ts",
                r#"
                    import { Inject } from "@msulak/anvil";
                    import { Heater } from "./heater";
                    @Inject
                    export class Pump {
                        constructor(private heater: Heater) {}
                    }
                "#,
            ),
            (
                "src/coffee.ts",
                r#"
                    import { Component } from "@msulak/anvil";
                    import { Pump } from "./pump";
                    import { Heater } from "./heater";
                    @Component({ modules: [] })
                    export abstract class CoffeeShop {
                        abstract pump(): Pump;
                        abstract heater(): Heater;
                    }
                "#,
            ),
        ],
    );

    // Stub `anvil` package so the resolver can find decorator imports.
    write_anvil_stub(&root);

    let resolver = ProjectResolver::new(None);
    let entry = root.join("src/coffee.ts");
    let graph = ProjectGraph::build_from_entry(&entry, &resolver).unwrap();

    // Three first-party files were reachable.
    let project_files: Vec<_> = graph
        .files
        .keys()
        .filter(|p| !p.to_string_lossy().contains("node_modules"))
        .collect();
    assert_eq!(project_files.len(), 3, "files: {:?}", graph.files.keys());

    // Every Key now carries an absolute path.
    for parsed in graph.files.values() {
        assert_all_keys_absolute(parsed);
    }

    // Sanity: Pump's Heater dep resolves to the same absolute path that
    // Heater's own self-binding sits under.
    let pump = graph
        .files
        .iter()
        .find(|(p, _)| p.ends_with("pump.ts"))
        .unwrap()
        .1;
    let heater = graph
        .files
        .iter()
        .find(|(p, _)| p.ends_with("heater.ts"))
        .unwrap();
    let pump_dep = match &pump.inject_classes[0].deps[0] {
        Key::Class { module, name } => {
            assert_eq!(name, "Heater");
            module.abs.clone()
        }
        Key::Set { .. } => panic!("expected Key::Class"),
    };
    let heater_self = match &heater.1.inject_classes[0].key {
        Key::Class { module, .. } => module.abs.clone(),
        Key::Set { .. } => panic!("expected Key::Class"),
    };
    assert_eq!(pump_dep, heater_self);
    assert!(Path::new(&pump_dep).ends_with("heater.ts"));
}

#[test]
fn tsconfig_paths_alias_resolves() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            (
                "tsconfig.json",
                r#"{
                    "compilerOptions": {
                        "baseUrl": ".",
                        "paths": { "@app/*": ["src/*"] }
                    }
                }"#,
            ),
            (
                "src/heater.ts",
                r#"
                    import { Singleton, Inject } from "@msulak/anvil";
                    @Inject
                    @Singleton
                    export class Heater { constructor() {} }
                "#,
            ),
            (
                "src/pump.ts",
                r#"
                    import { Inject } from "@msulak/anvil";
                    import { Heater } from "@app/heater";
                    @Inject
                    export class Pump { constructor(private heater: Heater) {} }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    let resolver = ProjectResolver::new(Some(root.join("tsconfig.json")));
    let entry = root.join("src/pump.ts");
    let graph = ProjectGraph::build_from_entry(&entry, &resolver).unwrap();

    // Heater was reached via the @app/* alias and parsed.
    assert!(graph.files.iter().any(|(p, _)| p.ends_with("heater.ts")));
    for parsed in graph.files.values() {
        assert_all_keys_absolute(parsed);
    }
}

#[test]
fn barrel_reexport_resolves_to_real_file() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            (
                "src/heater.ts",
                r#"
                    import { Inject } from "@msulak/anvil";
                    @Inject
                    export class Heater { constructor() {} }
                "#,
            ),
            // Barrel: re-exports Heater from a sibling.
            ("src/index.ts", "export { Heater } from \"./heater\";\n"),
            (
                "src/pump.ts",
                r#"
                    import { Inject } from "@msulak/anvil";
                    import { Heater } from "./index";
                    @Inject
                    export class Pump { constructor(private heater: Heater) {} }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    let resolver = ProjectResolver::new(None);
    let entry = root.join("src/pump.ts");
    let graph = ProjectGraph::build_from_entry(&entry, &resolver).unwrap();

    // The barrel re-export lands on `src/index.ts`, not on heater.ts. M2's
    // job is "stable absolute paths"; *following* the re-export to the
    // actual declaration is M3+ (handled by the binding-graph walker once
    // it sees `Heater` has no binding in `index.ts` and reports it).
    let pump = graph
        .files
        .iter()
        .find(|(p, _)| p.ends_with("pump.ts"))
        .unwrap()
        .1;
    let dep = match &pump.inject_classes[0].deps[0] {
        Key::Class { module, name } => {
            assert_eq!(name, "Heater");
            module.abs.clone()
        }
        Key::Set { .. } => panic!("expected Key::Class"),
    };
    assert!(
        dep.ends_with("index.ts"),
        "unexpected resolution target: {dep}"
    );

    for parsed in graph.files.values() {
        assert_all_keys_absolute(parsed);
    }
}

#[test]
fn node_modules_imports_resolve_but_are_not_walked() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            // Fake node_modules/some-lib package with a class export.
            (
                "node_modules/some-lib/package.json",
                r#"{ "main": "index.ts" }"#,
            ),
            (
                "node_modules/some-lib/index.ts",
                "
                    export class Logger {}
                ",
            ),
            (
                "src/pump.ts",
                r#"
                    import { Inject } from "@msulak/anvil";
                    import { Logger } from "some-lib";
                    @Inject
                    export class Pump { constructor(private log: Logger) {} }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    let resolver = ProjectResolver::new(None);
    let entry = root.join("src/pump.ts");
    let graph = ProjectGraph::build_from_entry(&entry, &resolver).unwrap();

    // Only `pump.ts` should be in the graph — `node_modules/some-lib` is
    // resolved but not parsed.
    assert_eq!(graph.files.len(), 1, "files: {:?}", graph.files.keys());
    let pump = graph.files.values().next().unwrap();
    assert_all_keys_absolute(pump);
    let dep = match &pump.inject_classes[0].deps[0] {
        Key::Class { module, .. } => (module.abs.clone(), module.original.clone()),
        Key::Set { .. } => panic!("expected Key::Class"),
    };
    assert!(
        dep.0.contains("node_modules") && dep.0.contains("some-lib"),
        "Logger dep should resolve into node_modules: {}",
        dep.0,
    );
    assert_eq!(
        dep.1.as_deref(),
        Some("some-lib"),
        "M2 must preserve the user's original specifier alongside abs",
    );
}

/// Write a minimal `node_modules/@msulak/anvil/index.ts` so resolver lookups for
/// decorator imports succeed in tests. The contents don't matter — we
/// don't recurse into `node_modules`.
fn write_anvil_stub(root: &Path) {
    let pkg = root.join("node_modules/@msulak/anvil");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "@msulak/anvil", "main": "index.ts" }"#,
    )
    .unwrap();
    std::fs::write(
        pkg.join("index.ts"),
        "export const Inject = (..._: any[]) => {};\n\
         export const Module = (..._: any[]) => {};\n\
         export const Provides = (..._: any[]) => {};\n\
         export const Component = (..._: any[]) => {};\n\
         export const Singleton = (..._: any[]) => {};\n\
         export const Binds = (..._: any[]) => {};\n",
    )
    .unwrap();
}
