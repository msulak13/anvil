//! Integration tests for `--config`-driven CLI invocations.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

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

fn write_tsdi_stub(root: &Path) {
    let pkg = root.join("node_modules/tsdi");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "tsdi", "main": "index.ts" }"#,
    )
    .unwrap();
    std::fs::write(
        pkg.join("index.ts"),
        "export const Inject = (..._: any[]) => {};\n\
         export const Module = (..._: any[]) => {};\n\
         export const Provides = (..._: any[]) => {};\n\
         export const Component = (..._: any[]) => {};\n\
         export const Singleton = (..._: any[]) => {};\n",
    )
    .unwrap();
}

const COFFEE_SRC: &[(&str, &str)] = &[
    (
        "src/heater.ts",
        r#"
            import { Inject } from "tsdi";
            @Inject
            export class Heater { constructor() {} }
        "#,
    ),
    (
        "src/pump.ts",
        r#"
            import { Inject } from "tsdi";
            import { Heater } from "./heater";
            @Inject
            export class Pump { constructor(private h: Heater) {} }
        "#,
    ),
    (
        "src/coffee-component.ts",
        r#"
            import { Component } from "tsdi";
            import { Pump } from "./pump";
            @Component({ modules: [] })
            export abstract class CoffeeShop { abstract pump(): Pump; }
        "#,
    ),
];

#[test]
fn build_with_config_file_emits_each_glob_match() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(&tmp, COFFEE_SRC);
    write_tsdi_stub(&root);
    std::fs::write(
        root.join("tsdi.config.json"),
        r#"{ "entries": ["src/*-component.ts"] }"#,
    )
    .unwrap();

    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("build")
        .arg("--config")
        .arg(root.join("tsdi.config.json"))
        .assert()
        .success()
        .stdout(contains("emitted").and(contains("coffee-component.tsdi.ts")));

    assert!(root.join("src/coffee-component.tsdi.ts").exists());
}

#[test]
fn check_with_package_json_tsdi_field() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(&tmp, COFFEE_SRC);
    write_tsdi_stub(&root);
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "demo", "tsdi": { "entries": ["src/*-component.ts"] } }"#,
    )
    .unwrap();

    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("check")
        .arg("--config")
        .arg(root.join("package.json"))
        .assert()
        .success()
        .stdout(contains("ok"));
}

#[test]
fn discovers_tsdi_config_from_cwd() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(&tmp, COFFEE_SRC);
    write_tsdi_stub(&root);
    std::fs::write(
        root.join("tsdi.config.json"),
        r#"{ "entries": ["src/*-component.ts"] }"#,
    )
    .unwrap();

    Command::cargo_bin("tsdi")
        .unwrap()
        .current_dir(&root)
        .arg("check")
        .assert()
        .success()
        .stdout(contains("ok"));
}
