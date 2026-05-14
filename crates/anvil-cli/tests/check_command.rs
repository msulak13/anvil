//! Integration tests for `anvil check`.
//!
//! Materializes small TypeScript projects into tempdirs, runs the CLI's
//! `check` subcommand, and asserts both the exit code and that each
//! intended diagnostic *kind* is named in stderr. Snapshot of the full
//! rendered miette output is intentionally avoided because it embeds
//! ANSI/Unicode borders that vary by terminal width.

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

fn write_anvil_stub(root: &Path) {
    let pkg = root.join("node_modules/@anvil-di/anvil");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "@anvil-di/anvil", "main": "index.ts" }"#,
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

#[test]
fn check_passes_on_valid_graph() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            (
                "src/heater.ts",
                r#"
                    import { Inject } from "@anvil-di/anvil";
                    @Inject
                    export class Heater { constructor() {} }
                "#,
            ),
            (
                "src/pump.ts",
                r#"
                    import { Inject } from "@anvil-di/anvil";
                    import { Heater } from "./heater";
                    @Inject
                    export class Pump { constructor(private h: Heater) {} }
                "#,
            ),
            (
                "src/coffee.ts",
                r#"
                    import { Component } from "@anvil-di/anvil";
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
    write_anvil_stub(&root);

    Command::cargo_bin("anvil")
        .unwrap()
        .arg("check")
        .arg("--entry")
        .arg(root.join("src/coffee.ts"))
        .assert()
        .success()
        .stdout(contains("ok"));
}

#[test]
fn check_reports_missing_binding() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            // Pump depends on a Heater that has no binding (no @Inject).
            ("src/heater.ts", "export class Heater {}\n"),
            (
                "src/pump.ts",
                r#"
                    import { Inject } from "@anvil-di/anvil";
                    import { Heater } from "./heater";
                    @Inject
                    export class Pump { constructor(private h: Heater) {} }
                "#,
            ),
            (
                "src/coffee.ts",
                r#"
                    import { Component } from "@anvil-di/anvil";
                    import { Pump } from "./pump";
                    @Component({ modules: [] })
                    export abstract class CoffeeShop {
                        abstract pump(): Pump;
                    }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    Command::cargo_bin("anvil")
        .unwrap()
        .arg("check")
        .arg("--entry")
        .arg(root.join("src/coffee.ts"))
        .assert()
        .failure()
        .stderr(contains("missing binding").and(contains("Heater")));
}

#[test]
fn check_reports_cycle() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            (
                "src/a.ts",
                r#"
                    import { Inject } from "@anvil-di/anvil";
                    import { B } from "./b";
                    @Inject
                    export class A { constructor(private b: B) {} }
                "#,
            ),
            (
                "src/b.ts",
                r#"
                    import { Inject } from "@anvil-di/anvil";
                    import { A } from "./a";
                    @Inject
                    export class B { constructor(private a: A) {} }
                "#,
            ),
            (
                "src/comp.ts",
                r#"
                    import { Component } from "@anvil-di/anvil";
                    import { A } from "./a";
                    @Component({ modules: [] })
                    export abstract class Comp { abstract a(): A; }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    Command::cargo_bin("anvil")
        .unwrap()
        .arg("check")
        .arg("--entry")
        .arg(root.join("src/comp.ts"))
        .assert()
        .failure()
        .stderr(contains("dependency cycle"));
}

#[test]
fn check_reports_duplicate_binding() {
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            ("src/heater.ts", "export class Heater {}\n"),
            (
                "src/m1.ts",
                r#"
                    import { Module, Provides } from "@anvil-di/anvil";
                    import { Heater } from "./heater";
                    @Module
                    export class M1 {
                        @Provides static provideHeater(): Heater { return new Heater(); }
                    }
                "#,
            ),
            (
                "src/m2.ts",
                r#"
                    import { Module, Provides } from "@anvil-di/anvil";
                    import { Heater } from "./heater";
                    @Module
                    export class M2 {
                        @Provides static provideHeater(): Heater { return new Heater(); }
                    }
                "#,
            ),
            (
                "src/comp.ts",
                r#"
                    import { Component } from "@anvil-di/anvil";
                    import { M1 } from "./m1";
                    import { M2 } from "./m2";
                    import { Heater } from "./heater";
                    @Component({ modules: [M1, M2] })
                    export abstract class Comp { abstract heater(): Heater; }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    Command::cargo_bin("anvil")
        .unwrap()
        .arg("check")
        .arg("--entry")
        .arg(root.join("src/comp.ts"))
        .assert()
        .failure()
        .stderr(contains("duplicate binding"));
}

#[test]
fn check_reports_scope_mismatch() {
    // M11: scope drives ownership. A @Singleton @Inject only contributes
    // to a @Singleton component's binding pool — for a non-@Singleton
    // component it's filtered out, and the entry point requesting it
    // surfaces as a missing binding instead of a scope-mismatch. Either
    // diagnostic is correct; the underlying configuration error
    // ("@Singleton class needs @Singleton component") is the same.
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            (
                "src/heater.ts",
                r#"
                    import { Inject, Singleton } from "@anvil-di/anvil";
                    @Inject
                    @Singleton
                    export class Heater { constructor() {} }
                "#,
            ),
            (
                "src/comp.ts",
                r#"
                    import { Component } from "@anvil-di/anvil";
                    import { Heater } from "./heater";
                    @Component({ modules: [] })
                    export abstract class Comp { abstract heater(): Heater; }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    Command::cargo_bin("anvil")
        .unwrap()
        .arg("check")
        .arg("--entry")
        .arg(root.join("src/comp.ts"))
        .assert()
        .failure()
        .stderr(contains("missing binding for Heater"));
}

#[test]
fn check_reports_scope_mismatch_for_singleton_provides() {
    // A @Singleton @Provides method does land in the binding pool (it
    // comes via a @Module the component lists explicitly), so the M3
    // ScopeMismatch rule still fires here — keeps the diagnostic
    // accessible for the canonical mis-scoped @Provides shape.
    let tmp = TempDir::new().unwrap();
    let root = write_project(
        &tmp,
        &[
            (
                "src/heater.ts",
                r"
                    export class Heater {}
                ",
            ),
            (
                "src/heater-module.ts",
                r#"
                    import { Module, Provides, Singleton } from "@anvil-di/anvil";
                    import { Heater } from "./heater";
                    @Module
                    export class HeaterModule {
                        @Singleton
                        @Provides
                        static heater(): Heater { return new Heater(); }
                    }
                "#,
            ),
            (
                "src/comp.ts",
                r#"
                    import { Component } from "@anvil-di/anvil";
                    import { HeaterModule } from "./heater-module";
                    import { Heater } from "./heater";
                    @Component({ modules: [HeaterModule] })
                    export abstract class Comp { abstract heater(): Heater; }
                "#,
            ),
        ],
    );
    write_anvil_stub(&root);

    Command::cargo_bin("anvil")
        .unwrap()
        .arg("check")
        .arg("--entry")
        .arg(root.join("src/comp.ts"))
        .assert()
        .failure()
        .stderr(contains("scope mismatch"));
}
