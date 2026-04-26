//! Golden-file integration test for the M4 emitter.
//!
//! Walks `tests/fixtures/<case>/input/`, copies it into a tempdir, runs
//! `tsdi build --entry <component>.ts`, and diffs the produced
//! `<component>.tsdi.ts` against `expected/<component>.tsdi.ts`.
//!
//! Set `BLESS=1` to overwrite the expected file with whatever the
//! emitter produced (use sparingly, and review the diff before
//! committing).
//!
//! M4 covers `01_simple_provides` only; later milestones add
//! `02_inject_ctor` (M6) and `03_singleton_scope` (M6).

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
         export const Singleton = (..._: any[]) => {};\n",
    )
    .unwrap();
}

#[test]
fn fixture_01_simple_provides() {
    let fixture = repo_root().join("tests/fixtures/01_simple_provides");
    let input = fixture.join("input");
    let expected_dir = fixture.join("expected");
    std::fs::create_dir_all(&expected_dir).unwrap();

    let tmp = TempDir::new().unwrap();
    let work = tmp.path().to_path_buf();
    copy_dir_recursive(&input, &work);
    write_tsdi_stub(&work);

    let entry = work.join("coffee-component.ts");
    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("build")
        .arg("--entry")
        .arg(&entry)
        .assert()
        .success();

    let produced = std::fs::read_to_string(work.join("coffee-component.tsdi.ts"))
        .expect("emitter should have written coffee-component.tsdi.ts");

    let expected_file = expected_dir.join("coffee-component.tsdi.ts");
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
        "emitted file does not match expected. Run with BLESS=1 to refresh."
    );
}
