//! Golden-file integration tests for the emitter.
//!
//! Each test copies a `tests/fixtures/<case>/input/` directory into a
//! tempdir, runs `tsdi build --entry <component>.ts`, and diffs the
//! produced `<component>.tsdi.ts` against `expected/<component>.tsdi.ts`.
//!
//! Set `BLESS=1` to overwrite the expected file with whatever the
//! emitter produced (use sparingly, and review the diff before
//! committing).
//!
//! Fixtures:
//! - `01_simple_provides` — `@Inject` ctor chain, `Scope::Unscoped` (M4).
//! - `03_singleton_scope` — `@Singleton @Inject` Heater shared by Pump (M6).
//! - `04_binds` — `@Binds` aliasing `ElectricHeater` to abstract `Heater` (M7).
//! - `05_subcomponent` — `@Subcomponent` `RequestComponent` nested inside
//!   `AppComponent`, inheriting `@Singleton Heater` from the parent (M8).

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// Repository root. Resolved relative to this test crate's manifest.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Recursively copy `src` into `dst`. Skips `expected/`-style directories.
fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Write the same `tsdi` runtime stub the unit tests use, so `oxc_resolver`
/// can resolve `import { ... } from "tsdi";`.
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
         export const Singleton = (..._: any[]) => {};\n\
         export const Binds = (..._: any[]) => {};\n\
         export const Subcomponent = (..._: any[]) => {};\n\
         export const IntoSet = (..._: any[]) => {};\n",
    )
    .unwrap();
}

/// Run one fixture: copy input into a tempdir, build, diff produced
/// against expected. `entry_file` is the component source filename
/// (e.g. `"coffee-component.ts"`); the produced output is the same name
/// with `.ts` swapped for `.tsdi.ts`.
fn run_fixture(case: &str, entry_file: &str) {
    let fixture = repo_root().join("tests/fixtures").join(case);
    let input = fixture.join("input");
    let expected_dir = fixture.join("expected");
    std::fs::create_dir_all(&expected_dir).unwrap();

    let tmp = TempDir::new().unwrap();
    let work = tmp.path().to_path_buf();
    copy_dir_recursive(&input, &work);
    write_tsdi_stub(&work);

    let entry = work.join(entry_file);
    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("build")
        .arg("--entry")
        .arg(&entry)
        .assert()
        .success();

    let out_name = entry_file.trim_end_matches(".ts").to_owned() + ".tsdi.ts";
    let produced = std::fs::read_to_string(work.join(&out_name))
        .unwrap_or_else(|_| panic!("emitter should have written {out_name}"));

    let expected_file = expected_dir.join(&out_name);
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&expected_file, &produced).unwrap();
        return;
    }
    assert!(
        expected_file.exists(),
        "expected file not found at {}; rerun with BLESS=1 to create it",
        expected_file.display()
    );
    let expected = std::fs::read_to_string(&expected_file).unwrap();
    assert_eq!(
        produced, expected,
        "emitted file does not match expected for fixture `{case}`. Run with BLESS=1 to refresh."
    );
}

#[test]
fn fixture_01_simple_provides() {
    run_fixture("01_simple_provides", "coffee-component.ts");
}

#[test]
fn fixture_03_singleton_scope() {
    run_fixture("03_singleton_scope", "coffee-component.ts");
}

#[test]
fn fixture_04_binds() {
    run_fixture("04_binds", "coffee-component.ts");
}

#[test]
fn fixture_05_subcomponent() {
    run_fixture("05_subcomponent", "app-component.ts");
}

#[test]
fn fixture_06_multibinding_set() {
    run_fixture("06_multibinding_set", "app-component.ts");
}

#[test]
fn fixture_07_interface_binding() {
    run_fixture("07_interface_binding", "coffee-component.ts");
}

#[test]
fn fixture_08_http_server() {
    run_fixture("08_http_server", "app-component.ts");
}

#[test]
fn fixture_09_express() {
    run_fixture("09_express", "app-component.ts");
}
