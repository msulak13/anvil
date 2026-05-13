//! Golden-file integration tests for `anvil-bellows`.
//!
//! Each test copies a `tests/fixtures/<case>/input/` directory into a
//! tempdir, runs `anvil-bellows --entry <dir> --output <file>`, and diffs
//! the produced `routes.module.ts` against `expected/routes.module.ts`.
//!
//! Set `BLESS=1` to overwrite the expected files (review the diff before
//! committing).
//!
//! Fixtures:
//! - `01_two_controllers` — two controller files with literal paths; snapshot +
//!   `tsc --noEmit` validation.
//! - `02_non_literal_arg` — one controller with a non-literal `@Controller`
//!   arg (skipped + diagnostic) and one good controller (included).

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

/// Path to the crate's `tests/fixtures/` directory.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Path to the monorepo root (two directories above this crate's manifest).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Path to `tsc` installed in the monorepo root's `node_modules/.bin/`.
fn tsc_bin() -> PathBuf {
    repo_root().join("node_modules/.bin/tsc")
}

/// Copy all files under `src` into `dst` recursively.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let from = e.path();
        let to = dst.join(e.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Write minimal package stubs so the generated `routes.module.ts` can be
/// type-checked without pulling in the full monorepo.
fn write_stubs(root: &Path) {
    // @msulak/anvil — provides Module, Provides, IntoSet decorators
    let anvil = root.join("node_modules/@msulak/anvil");
    std::fs::create_dir_all(&anvil).unwrap();
    std::fs::write(
        anvil.join("package.json"),
        r#"{ "name": "@msulak/anvil", "main": "index.ts", "types": "index.ts" }"#,
    )
    .unwrap();
    std::fs::write(
        anvil.join("index.ts"),
        // Simple any-typed stubs — Stage-3 decorator shape.
        "export const Module = (..._: any[]): any => {};\n\
         export const Provides = (..._: any[]): any => {};\n\
         export const IntoSet = (..._: any[]): any => {};\n",
    )
    .unwrap();

    // @msulak/anvil-bellows — provides RouteDefinition + controller decorators
    let bellows = root.join("node_modules/@msulak/anvil-bellows");
    std::fs::create_dir_all(&bellows).unwrap();
    std::fs::write(
        bellows.join("package.json"),
        r#"{ "name": "@msulak/anvil-bellows", "main": "index.ts", "types": "index.ts" }"#,
    )
    .unwrap();
    std::fs::write(
        bellows.join("index.ts"),
        "export interface RouteDefinition {\n\
           method: \"GET\" | \"POST\" | \"PUT\" | \"DELETE\" | \"PATCH\";\n\
           path: string;\n\
           handler: (req: unknown, res: unknown) => void | Promise<void>;\n\
         }\n\
         export const Controller = (..._: any[]): any => {};\n\
         export const Get = (..._: any[]): any => {};\n\
         export const Post = (..._: any[]): any => {};\n\
         export const Put = (..._: any[]): any => {};\n\
         export const Delete = (..._: any[]): any => {};\n\
         export const Patch = (..._: any[]): any => {};\n\
         export const Middleware = (..._: any[]): any => {};\n\
         export const Tag = (..._: any[]): any => {};\n\
         export const Returns = (..._: any[]): any => {};\n\
         export const Security = (..._: any[]): any => {};\n\
         export const Deprecated = (..._: any[]): any => {};\n",
    )
    .unwrap();

    // tsconfig.json at the work root
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["src"]
}"#,
    )
    .unwrap();
}

/// Run one fixture case and verify the output matches the expected file.
///
/// Returns the tempdir path so callers can run further checks (e.g. tsc).
fn run_fixture(case: &str) -> (TempDir, PathBuf) {
    let fixture = fixtures_dir().join(case);
    let input = fixture.join("input");
    let expected_dir = fixture.join("expected");
    std::fs::create_dir_all(&expected_dir).unwrap();

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    copy_dir(&input, &src);
    write_stubs(tmp.path());

    let output = src.join("routes.module.ts");

    Command::cargo_bin("anvil-bellows")
        .unwrap()
        .arg("--entry")
        .arg(&src)
        .arg("--output")
        .arg(&output)
        .assert()
        .success();

    let produced = std::fs::read_to_string(&output)
        .expect("anvil-bellows should have written routes.module.ts");

    let expected_file = expected_dir.join("routes.module.ts");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&expected_file, &produced).unwrap();
        return (tmp, output);
    }

    assert!(
        expected_file.exists(),
        "expected file missing at {}; run with BLESS=1 to create it",
        expected_file.display()
    );
    let expected = std::fs::read_to_string(&expected_file).unwrap();
    assert_eq!(
        produced, expected,
        "routes.module.ts does not match expected for fixture `{case}`. Run with BLESS=1 to refresh."
    );

    (tmp, output)
}

// ---------------------------------------------------------------------------
// Fixture 01 — two controllers with literal paths
// ---------------------------------------------------------------------------

#[test]
fn fixture_01_two_controllers_snapshot() {
    run_fixture("01_two_controllers");
}

#[test]
fn fixture_01_two_controllers_tsc() {
    let tsc = tsc_bin();
    if !tsc.exists() {
        eprintln!("skipping tsc check — tsc not found at {}", tsc.display());
        return;
    }

    let (tmp, _) = run_fixture("01_two_controllers");

    // Verify the generated file type-checks against the stubs.
    Command::new(tsc)
        .arg("--project")
        .arg(tmp.path().join("tsconfig.json"))
        .current_dir(tmp.path())
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Fixture 02 — non-literal @Controller arg: diagnostic + partial output
// ---------------------------------------------------------------------------

#[test]
fn fixture_02_non_literal_arg_snapshot() {
    let fixture = fixtures_dir().join("02_non_literal_arg");
    let input = fixture.join("input");
    let expected_dir = fixture.join("expected");
    std::fs::create_dir_all(&expected_dir).unwrap();

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    copy_dir(&input, &src);
    write_stubs(tmp.path());

    let output = src.join("routes.module.ts");

    // Exit code 1 because diagnostics were emitted.
    let assert = Command::cargo_bin("anvil-bellows")
        .unwrap()
        .arg("--entry")
        .arg(&src)
        .arg("--output")
        .arg(&output)
        .assert()
        .code(1)
        .stderr(contains("anvil-bellows"))
        .stderr(contains("Controller"))
        .stderr(contains("BASE"));

    let _ = assert;

    let produced = std::fs::read_to_string(&output)
        .expect("anvil-bellows should still write routes.module.ts for good controllers");

    // The bad controller's route must not appear.
    assert!(
        !produced.contains("BadController"),
        "BadController should have been skipped due to non-literal @Controller arg"
    );
    // The good controller's route must be present.
    assert!(
        produced.contains("HealthController"),
        "HealthController should be present in the output"
    );
    assert!(
        produced.contains("healthControllerGetPing"),
        "health ping route should be present"
    );

    let expected_file = expected_dir.join("routes.module.ts");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&expected_file, &produced).unwrap();
        return;
    }
    if expected_file.exists() {
        let expected = std::fs::read_to_string(&expected_file).unwrap();
        assert_eq!(produced, expected, "run with BLESS=1 to refresh");
    }
}
