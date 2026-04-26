//! Integration tests for `tsdi explain`.

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

#[test]
fn explain_traces_inject_chain() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
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
                "src/coffee.ts",
                r#"
                    import { Component } from "tsdi";
                    import { Pump } from "./pump";
                    @Component({ modules: [] })
                    export abstract class CoffeeShop {
                        abstract pump(): Pump;
                    }
                "#,
            ),
        ],
    );
    write_tsdi_stub(&root);

    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("explain")
        .arg("Pump")
        .arg("--entry")
        .arg(root.join("src/coffee.ts"))
        .assert()
        .success()
        .stdout(
            contains("Pump@")
                .and(contains("InjectCtor"))
                .and(contains("Heater@")),
        );
}

#[test]
fn explain_unknown_key_errors() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[(
            "src/coffee.ts",
            r#"
                    import { Component } from "tsdi";
                    @Component({ modules: [] })
                    export abstract class CoffeeShop {}
                "#,
        )],
    );
    write_tsdi_stub(&root);

    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("explain")
        .arg("Doesnotexist")
        .arg("--entry")
        .arg(root.join("src/coffee.ts"))
        .assert()
        .failure()
        .stderr(contains("no binding named"));
}
