//! Golden-file integration tests for `anvil-bellows-openapi`.
//!
//! Each test copies a `tests/fixtures/<case>/input/` directory into a tempdir,
//! runs the CLI, and diffs the produced JSON against
//! `expected/openapi.json`.
//!
//! Set `BLESS=1` to overwrite the expected files.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

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

struct FixtureRun {
    _tmp: TempDir,
    output: String,
}

fn run_fixture(name: &str, extra_args: &[&str]) -> FixtureRun {
    let fixture = fixtures_dir().join(name);
    let tmp = TempDir::new().unwrap();
    let input_dst = tmp.path().join("input");
    copy_dir(&fixture.join("input"), &input_dst);

    let output_path = tmp.path().join("openapi.json");
    let config_path = input_dst.join("openapi.config.json");

    let mut cmd = Command::cargo_bin("anvil-bellows-openapi").unwrap();
    cmd.arg("--entry")
        .arg(&input_dst)
        .arg("--output")
        .arg(&output_path);
    if config_path.exists() {
        cmd.arg("--config").arg(&config_path);
    }
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.assert().success();

    let output = std::fs::read_to_string(&output_path).unwrap();
    FixtureRun { _tmp: tmp, output }
}

fn bless_or_assert(fixture_name: &str, file_name: &str, actual: &str) {
    let expected_path = fixtures_dir()
        .join(fixture_name)
        .join("expected")
        .join(file_name);
    if std::env::var("BLESS").is_ok() {
        std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
        std::fs::write(&expected_path, actual).unwrap();
    } else {
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|_| {
            panic!(
                "expected file not found: {}; run with BLESS=1",
                expected_path.display()
            )
        });
        assert_eq!(
            actual,
            expected.as_str(),
            "OpenAPI output mismatch for {fixture_name}/{file_name}; run with BLESS=1 to update"
        );
    }
}

#[test]
fn fixture_01_basic_routes_snapshot() {
    let run = run_fixture("01_basic_routes", &[]);
    // Sanity checks on the live output.
    let doc: serde_json::Value = serde_json::from_str(&run.output).expect("valid JSON");
    assert_eq!(doc["openapi"], "3.1.0");
    assert!(doc["paths"]["/users"].is_object(), "missing /users path");
    assert!(
        doc["paths"]["/users/{id}"].is_object(),
        "missing /users/{{id}} path"
    );
    bless_or_assert("01_basic_routes", "openapi.json", &run.output);
}

#[test]
fn fixture_01_basic_routes_has_security() {
    let run = run_fixture("01_basic_routes", &[]);
    let doc: serde_json::Value = serde_json::from_str(&run.output).unwrap();
    // @Security("bearerAuth") with no config file → diagnostic warning but still emitted.
    let get_list = &doc["paths"]["/users"]["get"];
    let security = &get_list["security"];
    assert!(security.is_array(), "expected security array on GET /users");
}

#[test]
fn fixture_01_basic_routes_deprecated_route() {
    let run = run_fixture("01_basic_routes", &[]);
    let doc: serde_json::Value = serde_json::from_str(&run.output).unwrap();
    let delete_op = &doc["paths"]["/users/{id}"]["delete"];
    assert_eq!(
        delete_op["deprecated"], true,
        "DELETE /:id should be deprecated"
    );
    // @Returns(204) should add a 204 response.
    assert!(
        delete_op["responses"]["204"].is_object(),
        "expected 204 response"
    );
}

#[test]
fn fixture_01_basic_routes_yaml_format() {
    let fixture = fixtures_dir().join("01_basic_routes");
    let tmp = TempDir::new().unwrap();
    let input_dst = tmp.path().join("input");
    copy_dir(&fixture.join("input"), &input_dst);
    let output_path = tmp.path().join("openapi.yaml");
    let config_path = input_dst.join("openapi.config.json");

    Command::cargo_bin("anvil-bellows-openapi")
        .unwrap()
        .arg("--entry")
        .arg(&input_dst)
        .arg("--output")
        .arg(&output_path)
        .arg("--format")
        .arg("yaml")
        .arg("--config")
        .arg(&config_path)
        .assert()
        .success();

    let yaml = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        yaml.contains("openapi: 3.1.0"),
        "YAML should contain openapi version"
    );
    assert!(yaml.contains("/users"), "YAML should contain /users path");
}

// ---------------------------------------------------------------------------
// Fixture 02 — content-type codecs: the two-arg Body<S, C> (request) and
//              Produces<S, C> (response) end to end through the real CLI,
//              verifying their codecs' contentType literals make it into the
//              generated OpenAPI document instead of the application/json
//              default.
// ---------------------------------------------------------------------------

#[test]
fn fixture_02_content_type_codecs_snapshot() {
    let run = run_fixture("02_content_type_codecs", &[]);
    let doc: serde_json::Value = serde_json::from_str(&run.output).expect("valid JSON");
    assert_eq!(doc["openapi"], "3.1.0");
    assert!(
        doc["paths"]["/webhooks/gather"].is_object(),
        "missing /webhooks/gather path"
    );
    bless_or_assert("02_content_type_codecs", "openapi.json", &run.output);
}

#[test]
fn fixture_02_plain_body_still_defaults_to_json() {
    let run = run_fixture("02_content_type_codecs", &[]);
    let doc: serde_json::Value = serde_json::from_str(&run.output).unwrap();
    let request_body = &doc["paths"]["/webhooks/greeting"]["post"]["requestBody"];
    assert!(
        request_body["content"]["application/json"].is_object(),
        "single-arg Body<S> should still default to application/json"
    );
    assert!(request_body["content"]["application/xml"].is_null());
}

#[test]
fn fixture_02_two_arg_body_uses_resolved_request_codec_content_type() {
    let run = run_fixture("02_content_type_codecs", &[]);
    let doc: serde_json::Value = serde_json::from_str(&run.output).unwrap();
    let request_body = &doc["paths"]["/webhooks/gather"]["post"]["requestBody"];
    assert_eq!(
        request_body["content"]["application/xml"]["schema"]["$ref"],
        "#/components/schemas/GatherCallbackSchema"
    );
    assert!(
        request_body["content"]["application/json"].is_null(),
        "should not also list application/json once the codec's content type is resolved"
    );
}

#[test]
fn fixture_02_produces_uses_resolved_response_codec_content_type() {
    let run = run_fixture("02_content_type_codecs", &[]);
    let doc: serde_json::Value = serde_json::from_str(&run.output).unwrap();
    let response_content = &doc["paths"]["/webhooks/say"]["post"]["responses"]["200"]["content"];
    assert_eq!(
        response_content["application/xml"]["schema"]["$ref"],
        "#/components/schemas/TwimlResponseSchema"
    );
    assert!(
        response_content["application/json"].is_null(),
        "should not also document application/json once Produces's codec is resolved"
    );
}
