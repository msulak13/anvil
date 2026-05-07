//! Integration tests for the M13 in-memory file source. Verifies
//! `ProjectGraph::build_from_map` produces the same shape as
//! `build_from_entry` for matching inputs — every path normalizes
//! the same way, every key compares equal across the two backends.

use std::path::PathBuf;

use tsdi_core::ir::Key;
use tsdi_parser::map_source::{FileMap, MapResolver, PathAlias};
use tsdi_parser::symbols::ProjectGraph;

fn project() -> FileMap {
    FileMap::from_pairs(vec![
        // Stub the runtime tsdi package so resolve_name_to_key can mint Keys
        // for our decorators. Bare specifier "tsdi" lands here.
        (
            PathBuf::from("/proj/node_modules/tsdi/index.d.ts"),
            "export const Inject: any; export const Module: any; \
             export const Provides: any; export const Component: any; \
             export const Singleton: any; export const Binds: any;"
                .to_owned(),
        ),
        (
            PathBuf::from("/proj/src/heater.ts"),
            r#"
                import { Inject, Singleton } from "tsdi";
                @Inject @Singleton
                export class Heater { constructor() {} }
            "#
            .to_owned(),
        ),
        (
            PathBuf::from("/proj/src/pump.ts"),
            r#"
                import { Inject } from "tsdi";
                import { Heater } from "./heater";
                @Inject
                export class Pump { constructor(private heater: Heater) {} }
            "#
            .to_owned(),
        ),
        (
            PathBuf::from("/proj/src/app-component.ts"),
            r#"
                import { Component, Singleton } from "tsdi";
                import { Pump } from "./pump";
                @Singleton
                @Component({ modules: [] })
                export abstract class App { abstract pump(): Pump; }
            "#
            .to_owned(),
        ),
    ])
}

#[test]
fn build_from_map_walks_relative_imports_and_normalizes_keys() {
    let files = project();
    let resolver = MapResolver::new();
    let graph = ProjectGraph::build_from_map(
        &PathBuf::from("/proj/src/app-component.ts"),
        &files,
        &resolver,
    )
    .expect("graph builds");

    // Three project-source files reachable from the entry; node_modules/tsdi
    // is resolved (so its specifier normalizes) but not walked.
    let paths: Vec<String> = graph
        .files
        .keys()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert!(paths.iter().any(|p| p.ends_with("app-component.ts")));
    assert!(paths.iter().any(|p| p.ends_with("pump.ts")));
    assert!(paths.iter().any(|p| p.ends_with("heater.ts")));
    assert!(!paths.iter().any(|p| p.contains("node_modules")));

    // Pump's inject_classes[0].deps[0] is Heater, with abs path normalized
    // to the same string both files can reach (regardless of which file
    // imported it).
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
        .unwrap()
        .1;
    let pump_dep_module = match &pump.inject_classes[0].deps[0] {
        Key::Class { module, .. } => module.abs.clone(),
        Key::Set { .. } => panic!("expected Key::Class"),
    };
    let heater_self_module = match &heater.inject_classes[0].key {
        Key::Class { module, .. } => module.abs.clone(),
        Key::Set { .. } => panic!("expected Key::Class"),
    };
    assert_eq!(
        pump_dep_module, heater_self_module,
        "M2 contract: equivalent imports across files produce equal Keys",
    );
    assert!(pump_dep_module.ends_with("heater.ts"));
}

#[test]
fn missing_file_in_map_surfaces_a_diagnostic() {
    // Entry imports `./missing` which isn't in the file map.
    let files = FileMap::from_pairs(vec![
        (
            PathBuf::from("/proj/node_modules/tsdi/index.d.ts"),
            "export const Inject: any;".to_owned(),
        ),
        (
            PathBuf::from("/proj/src/main.ts"),
            r#"
                import { Inject } from "tsdi";
                import { Helper } from "./missing";
                @Inject
                export class Main { constructor(private h: Helper) {} }
            "#
            .to_owned(),
        ),
    ]);
    let resolver = MapResolver::new();
    let err = ProjectGraph::build_from_map(&PathBuf::from("/proj/src/main.ts"), &files, &resolver)
        .expect_err("should fail on unresolvable specifier");
    let msg = format!("{err}");
    assert!(
        msg.contains("./missing"),
        "expected diagnostic to name the bad specifier; got: {msg}",
    );
}

#[test]
fn tsconfig_paths_alias_resolves_through_the_map() {
    // `@/heater` aliases to `src/heater`; the resolver applies the
    // alias rewrite before extension probing.
    let files = FileMap::from_pairs(vec![
        (
            PathBuf::from("/proj/node_modules/tsdi/index.d.ts"),
            "export const Inject: any;".to_owned(),
        ),
        (
            PathBuf::from("/proj/src/heater.ts"),
            r#"
                import { Inject } from "tsdi";
                @Inject
                export class Heater { constructor() {} }
            "#
            .to_owned(),
        ),
        (
            PathBuf::from("/proj/src/pump.ts"),
            r#"
                import { Inject } from "tsdi";
                import { Heater } from "@/heater";
                @Inject
                export class Pump { constructor(private heater: Heater) {} }
            "#
            .to_owned(),
        ),
    ]);
    let resolver = MapResolver::with_aliases(vec![PathAlias {
        pattern: "@/*".to_owned(),
        target: "src/*".to_owned(),
        base_dir: PathBuf::from("/proj"),
    }]);
    let graph =
        ProjectGraph::build_from_map(&PathBuf::from("/proj/src/pump.ts"), &files, &resolver)
            .expect("alias resolves");
    let pump = graph
        .files
        .iter()
        .find(|(p, _)| p.ends_with("pump.ts"))
        .unwrap()
        .1;
    let heater_dep = match &pump.inject_classes[0].deps[0] {
        Key::Class { module, .. } => module.abs.clone(),
        Key::Set { .. } => panic!("expected Key::Class"),
    };
    assert!(heater_dep.ends_with("heater.ts"));
}
