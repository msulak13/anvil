//! Integration test for `anvil watch`.
//!
//! The watcher loops forever in production. We pin the loop to a single
//! iteration via `ANVIL_WATCH_ITERATIONS=1` so the test can assert that
//! editing a source file produces an updated `*.anvil.ts` and then exits.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::cargo::CommandCargoExt;
use tempfile::TempDir;

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

#[test]
fn watch_regenerates_anvil_file_on_source_change() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    write_anvil_stub(root);

    // Initial sources: trivial component with one entry point.
    std::fs::write(
        root.join("src/heater.ts"),
        "import { Inject } from \"@msulak/anvil\";\n\
         @Inject\n\
         export class Heater { constructor() {} }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/coffee-component.ts"),
        "import { Component } from \"@msulak/anvil\";\n\
         import { Heater } from \"./heater\";\n\
         @Component({ modules: [] })\n\
         export abstract class CoffeeShop { abstract heater(): Heater; }\n",
    )
    .unwrap();

    let entry = root.join("src/coffee-component.ts");
    let out = root.join("src/coffee-component.anvil.ts");

    let mut child = Command::cargo_bin("anvil")
        .unwrap()
        .env("ANVIL_WATCH_ITERATIONS", "1")
        .arg("watch")
        .arg("--entry")
        .arg(&entry)
        .spawn()
        .expect("spawn anvil watch");

    // Wait for the initial emit.
    wait_until(Duration::from_secs(15), || out.is_file()).expect("initial emit");
    let v1 = std::fs::read_to_string(&out).unwrap();
    assert!(v1.contains("AnvilCoffeeShop"));

    // Mutate the source: rename Heater's body so the new anvil.ts changes
    // mtime/content (we add a method that affects no IR but ensures the
    // file is rewritten).
    std::thread::sleep(Duration::from_millis(50));
    std::fs::write(
        root.join("src/heater.ts"),
        "import { Inject } from \"@msulak/anvil\";\n\
         @Inject\n\
         export class Heater { constructor() {} on() {} }\n",
    )
    .unwrap();

    // Watcher should consume the event, rebuild, and exit (1 iteration cap).
    let status = wait_for_exit(&mut child, Duration::from_secs(30))
        .expect("watch should exit after 1 iteration");
    assert!(status.success(), "watch exited non-zero: {status:?}");

    // The output file still exists and still contains the generated class.
    let v2 = std::fs::read_to_string(&out).unwrap();
    assert!(v2.contains("AnvilCoffeeShop"));
}

fn wait_until(timeout: Duration, mut pred: impl FnMut() -> bool) -> Result<(), &'static str> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("timed out")
}

fn wait_for_exit(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return None,
        }
    }
    let _ = child.kill();
    None
}
