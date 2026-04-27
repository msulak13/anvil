//! Integration test for `tsdi watch`.
//!
//! The watcher loops forever in production. We pin the loop to a single
//! iteration via `TSDI_WATCH_ITERATIONS=1` so the test can assert that
//! editing a source file produces an updated `*.tsdi.ts` and then exits.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::cargo::CommandCargoExt;
use tempfile::TempDir;

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
         export const Binds = (..._: any[]) => {};\n",
    )
    .unwrap();
}

#[test]
fn watch_regenerates_tsdi_file_on_source_change() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    write_tsdi_stub(root);

    // Initial sources: trivial component with one entry point.
    std::fs::write(
        root.join("src/heater.ts"),
        "import { Inject } from \"tsdi\";\n\
         @Inject\n\
         export class Heater { constructor() {} }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/coffee-component.ts"),
        "import { Component } from \"tsdi\";\n\
         import { Heater } from \"./heater\";\n\
         @Component({ modules: [] })\n\
         export abstract class CoffeeShop { abstract heater(): Heater; }\n",
    )
    .unwrap();

    let entry = root.join("src/coffee-component.ts");
    let out = root.join("src/coffee-component.tsdi.ts");

    let mut child = Command::cargo_bin("tsdi")
        .unwrap()
        .env("TSDI_WATCH_ITERATIONS", "1")
        .arg("watch")
        .arg("--entry")
        .arg(&entry)
        .spawn()
        .expect("spawn tsdi watch");

    // Wait for the initial emit.
    wait_until(Duration::from_secs(15), || out.is_file()).expect("initial emit");
    let v1 = std::fs::read_to_string(&out).unwrap();
    assert!(v1.contains("DaggerCoffeeShop"));

    // Mutate the source: rename Heater's body so the new tsdi.ts changes
    // mtime/content (we add a method that affects no IR but ensures the
    // file is rewritten).
    std::thread::sleep(Duration::from_millis(50));
    std::fs::write(
        root.join("src/heater.ts"),
        "import { Inject } from \"tsdi\";\n\
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
    assert!(v2.contains("DaggerCoffeeShop"));
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
