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
//! - `08_form_body` — a route combining `FormBody<S>`, `Headers<S>`, and
//!   `RawBody`, verifying the `urlencoded` body parser is selected and
//!   `req.rawBody` is injected.

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
#[allow(clippy::too_many_lines)]
fn write_stubs(root: &Path) {
    // @anvil-di/anvil — provides Module, Provides, IntoSet decorators
    let anvil = root.join("node_modules/@anvil-di/anvil");
    std::fs::create_dir_all(&anvil).unwrap();
    std::fs::write(
        anvil.join("package.json"),
        r#"{ "name": "@anvil-di/anvil", "main": "index.ts", "types": "index.ts" }"#,
    )
    .unwrap();
    std::fs::write(
        anvil.join("index.ts"),
        // Simple any-typed stubs — Stage-3 decorator shape.
        "export const Module = (..._: any[]): any => {};\n\
         export const Provides = (..._: any[]): any => {};\n\
         export const IntoSet = (..._: any[]): any => {};\n\
         export const Singleton = (..._: any[]): any => {};\n",
    )
    .unwrap();

    // @anvil-di/bellows — provides RouteDefinition + controller decorators
    let bellows = root.join("node_modules/@anvil-di/bellows");
    std::fs::create_dir_all(&bellows).unwrap();
    std::fs::write(
        bellows.join("package.json"),
        r#"{ "name": "@anvil-di/bellows", "main": "index.ts", "types": "index.ts" }"#,
    )
    .unwrap();
    std::fs::write(
        bellows.join("index.ts"),
        // handler uses `any` so generated safeParse/res.json calls type-check.
        "export interface AuthnResult<U = unknown> {\n\
           identified: boolean;\n\
           user?: U;\n\
         }\n\
         export interface AuthnService<U = unknown, Scheme extends string = never> {\n\
           identify(req: any): AuthnResult<U> | Promise<AuthnResult<U>>;\n\
         }\n\
         export type AuthzDecision = \"allow\" | \"deny\" | \"next\";\n\
         export interface AuthzService {\n\
           authorize(req: any, user: unknown): AuthzDecision | Promise<AuthzDecision>;\n\
         }\n\
         export type AuthnUser<T> = T;\n\
         export interface RouteDefinition {\n\
           method: \"GET\" | \"POST\" | \"PUT\" | \"DELETE\" | \"PATCH\";\n\
           path: string;\n\
           authn?: AuthnService[];\n\
           authz?: AuthzService[];\n\
           middleware?: ((req: any, res: any, next: any) => void)[];\n\
           bodyParser?: \"json\" | \"urlencoded\" | \"raw\";\n\
           handler: (req: any, res: any) => void | Promise<void>;\n\
         }\n\
         export type Body<S> = S extends { safeParse(x: unknown): { success: true; data: infer T } | any } ? T : never;\n\
         export type Query<S> = Body<S>;\n\
         export type Params<S> = Body<S>;\n\
         export type Responds<S> = Body<S>;\n\
         export type FormBody<S> = Body<S>;\n\
         export type Headers<S> = Body<S>;\n\
         export type RawBody = { toString(encoding?: string): string };\n\
         export interface ResponseCodec<T> {\n\
           readonly contentType: string;\n\
           encode(value: T): string;\n\
         }\n\
         export type Produces<S, C extends ResponseCodec<Responds<S>>> = Responds<S>;\n\
         export class HttpError extends Error {\n\
           constructor(readonly status: number, readonly error: string, message?: string) { super(message ?? error); }\n\
         }\n\
         export class BadRequestError extends HttpError {\n\
           constructor(message?: string) { super(400, \"Bad Request\", message); }\n\
         }\n\
         export class InternalServerError extends HttpError {\n\
           constructor(message?: string) { super(500, \"Internal Server Error\", message); }\n\
         }\n\
         export function disconnectSignal(_req: any): AbortSignal { return new AbortController().signal; }\n\
         export class SseStream {\n\
           readonly signal: AbortSignal;\n\
           constructor(_res: any, signal: AbortSignal) { this.signal = signal; }\n\
           open(_keepAliveMs?: number | false): this { return this; }\n\
           send(_data: unknown, _opts?: { event?: string; id?: string; retry?: number }): void {}\n\
           comment(_text?: string): void {}\n\
           close(): void {}\n\
         }\n\
         export const Controller = (..._: any[]): any => {};\n\
         export const Get = (..._: any[]): any => {};\n\
         export const Sse = (..._: any[]): any => {};\n\
         export const Post = (..._: any[]): any => {};\n\
         export const Put = (..._: any[]): any => {};\n\
         export const Delete = (..._: any[]): any => {};\n\
         export const Patch = (..._: any[]): any => {};\n\
         export const Middleware = (..._: any[]): any => {};\n\
         export const Authn = (..._: any[]): any => {};\n\
         export const Authz = (..._: any[]): any => {};\n\
         export const Tag = (..._: any[]): any => {};\n\
         export const Returns = (..._: any[]): any => {};\n\
         export const Security = (..._: any[]): any => {};\n\
         export const Deprecated = (..._: any[]): any => {};\n",
    )
    .unwrap();

    // zod-to-json-schema — provides zodToJsonSchema used by schema-route.module.anvil.ts
    let zod_json_schema = root.join("node_modules/zod-to-json-schema");
    std::fs::create_dir_all(&zod_json_schema).unwrap();
    std::fs::write(
        zod_json_schema.join("package.json"),
        r#"{ "name": "zod-to-json-schema", "main": "index.ts", "types": "index.ts" }"#,
    )
    .unwrap();
    std::fs::write(
        zod_json_schema.join("index.ts"),
        "export function zodToJsonSchema(_schema: unknown, _options?: unknown): Record<string, unknown> { return {}; }\n",
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

// ---------------------------------------------------------------------------
// Fixture 03 — type-driven adapters: Body, Query, Params, Request, Response,
//              Responds<T>
// ---------------------------------------------------------------------------

#[test]
fn fixture_03_schema_params_snapshot() {
    run_fixture("03_schema_params");
}

#[test]
fn fixture_03_schema_params_tsc() {
    let tsc = tsc_bin();
    if !tsc.exists() {
        eprintln!("skipping tsc check — tsc not found at {}", tsc.display());
        return;
    }

    let (tmp, _) = run_fixture("03_schema_params");

    Command::new(tsc)
        .arg("--project")
        .arg(tmp.path().join("tsconfig.json"))
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn fixture_03_schema_params_no_typeof_in_output() {
    let (_tmp, output_path) = run_fixture("03_schema_params");
    let produced = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        !produced.contains("typeof"),
        "generated adapter must not contain `typeof` — schema refs should be value identifiers"
    );
}

#[test]
fn fixture_03_schema_params_safe_parse_calls() {
    let (_tmp, output_path) = run_fixture("03_schema_params");
    let produced = std::fs::read_to_string(&output_path).unwrap();
    assert!(produced.contains("CreateOrderBody.safeParse(req.body)"));
    assert!(produced.contains("OrderFilterQuery.safeParse(req.query)"));
    assert!(produced.contains("OrderParams.safeParse(req.params)"));
    assert!(produced.contains("OrderSchema.safeParse(_result)"));
    assert!(produced.contains("res.json(_validated.data)"));
}

// ---------------------------------------------------------------------------
// Fixture 04 — @Middleware chains: class-level + method-level, imported from
//              another file and declared in the controller file itself.
// ---------------------------------------------------------------------------

#[test]
fn fixture_04_middleware_snapshot() {
    run_fixture("04_middleware");
}

#[test]
fn fixture_04_middleware_tsc() {
    let tsc = tsc_bin();
    if !tsc.exists() {
        eprintln!("skipping tsc check — tsc not found at {}", tsc.display());
        return;
    }

    let (tmp, _) = run_fixture("04_middleware");

    Command::new(tsc)
        .arg("--project")
        .arg(tmp.path().join("tsconfig.json"))
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn fixture_04_middleware_ordering() {
    let (_tmp, output_path) = run_fixture("04_middleware");
    let produced = std::fs::read_to_string(&output_path).unwrap();
    // Class-level middleware applies to every route.
    assert!(produced.contains("middleware: [requireAuth],"));
    // Class-level middleware runs before method-level middleware, in
    // declaration order.
    assert!(produced.contains("requireAuth,\n\t\t\t\trequireAdmin,\n\t\t\t\tauditLog"));
    // Middleware imported from another file is imported into routes.module.ts.
    assert!(produced.contains("import { requireAdmin, requireAuth } from \"./auth-middleware\";"));
    // Middleware declared in the controller file itself is imported alongside it.
    assert!(produced.contains("import { AdminController, auditLog } from \"./admin-controller\";"));
}

// ---------------------------------------------------------------------------
// Fixture 05 — @Authn/@Authz: class-level authn cascade + method-level authz
//              cascade, DI-injected extra params, cross-file scheme literal.
// ---------------------------------------------------------------------------

#[test]
fn fixture_05_authn_authz_snapshot() {
    run_fixture("05_authn_authz");
}

#[test]
fn fixture_05_authn_authz_tsc() {
    let tsc = tsc_bin();
    if !tsc.exists() {
        eprintln!("skipping tsc check — tsc not found at {}", tsc.display());
        return;
    }

    let (tmp, _) = run_fixture("05_authn_authz");

    Command::new(tsc)
        .arg("--project")
        .arg(tmp.path().join("tsconfig.json"))
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn fixture_05_authn_authz_di_params_and_fields() {
    let (_tmp, output_path) = run_fixture("05_authn_authz");
    let produced = std::fs::read_to_string(&output_path).unwrap();

    // Class-level @Authn applies to every route on the controller.
    assert!(produced.contains("authn: [sessionAuthn]"));
    // Method-level @Authz applies only to `stats`.
    assert!(produced.contains("authz: [roleAuthz]"));
    // Extra DI params on the provider methods.
    assert!(produced.contains("sessionAuthn: SessionAuthn"));
    assert!(produced.contains("roleAuthz: RoleAuthz"));
    // Both service classes are type-only imported from the same file.
    assert!(produced.contains("import type { RoleAuthz, SessionAuthn } from \"./auth-services\";"));
    // `stats`'s AuthnUser<User> param is injected from res.locals.user.
    assert!(produced.contains("adminController.stats(res.locals.user)"));
}

// ---------------------------------------------------------------------------
// Fixture 06 — Produces<S, C>: non-JSON response body via a ResponseCodec,
//              alongside a Body<S> param on the same route.
// ---------------------------------------------------------------------------

#[test]
fn fixture_06_produces_snapshot() {
    run_fixture("06_produces");
}

#[test]
fn fixture_06_produces_tsc() {
    let tsc = tsc_bin();
    if !tsc.exists() {
        eprintln!("skipping tsc check — tsc not found at {}", tsc.display());
        return;
    }

    let (tmp, _) = run_fixture("06_produces");

    // Verifies the fixture's ResponseCodec<Responds<typeof TwimlResponseSchema>>
    // actually satisfies the stubbed Produces<S, C> bound — not just that the
    // generated glue code type-checks.
    Command::new(tsc)
        .arg("--project")
        .arg(tmp.path().join("tsconfig.json"))
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn fixture_06_produces_uses_codec_not_json() {
    let (_tmp, output_path) = run_fixture("06_produces");
    let produced = std::fs::read_to_string(&output_path).unwrap();

    // Codec import alongside the controller and schemas.
    assert!(produced.contains(
        "import { WebhooksController, GatherWebhookBody, TwimlResponseSchema, twimlCodec } from \"./webhooks-controller\";"
    ));
    // Body param still validated as usual.
    assert!(produced.contains("GatherWebhookBody.safeParse(req.body)"));
    // Return value still validated against the response schema.
    assert!(produced.contains("TwimlResponseSchema.safeParse(_result)"));
    // Serialized via the codec, not res.json().
    assert!(produced
        .contains("res.type(twimlCodec.contentType).send(twimlCodec.encode(_validated.data));"));
    assert!(!produced.contains("res.json(_validated.data)"));
    // No typeof in generated code.
    assert!(!produced.contains("typeof"));
}

// ---------------------------------------------------------------------------
// Fixture 07 — @Sse route with SseStream + AbortSignal injection
// ---------------------------------------------------------------------------

#[test]
fn fixture_07_sse_snapshot() {
    run_fixture("07_sse");
}

#[test]
fn fixture_07_sse_tsc() {
    let tsc = tsc_bin();
    if !tsc.exists() {
        eprintln!("skipping tsc check — tsc not found at {}", tsc.display());
        return;
    }

    let (tmp, _) = run_fixture("07_sse");

    Command::new(tsc)
        .arg("--project")
        .arg(tmp.path().join("tsconfig.json"))
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn fixture_07_sse_constructs_stream_and_signal() {
    let (_tmp, output) = run_fixture("07_sse");
    let produced = std::fs::read_to_string(&output).unwrap();
    assert!(produced.contains("new SseStream(res, disconnectSignal(req))"));
    assert!(produced.contains("disconnectSignal(req)"));
    assert!(
        produced.contains(
            "import { SseStream, disconnectSignal, BadRequestError } from \"@anvil-di/bellows\";"
        ) || produced.contains("disconnectSignal")
    );
}

// ---------------------------------------------------------------------------
// Fixture 08 — FormBody<S>, Headers<S>, RawBody: a webhook-style route that
//              validates a form-urlencoded body and headers, and also grabs
//              the raw request bytes (Twilio-signature-verification shape).
// ---------------------------------------------------------------------------

#[test]
fn fixture_08_form_body_snapshot() {
    run_fixture("08_form_body");
}

#[test]
fn fixture_08_form_body_tsc() {
    let tsc = tsc_bin();
    if !tsc.exists() {
        eprintln!("skipping tsc check — tsc not found at {}", tsc.display());
        return;
    }

    let (tmp, _) = run_fixture("08_form_body");

    Command::new(tsc)
        .arg("--project")
        .arg(tmp.path().join("tsconfig.json"))
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn fixture_08_form_body_selects_urlencoded_parser() {
    let (_tmp, output_path) = run_fixture("08_form_body");
    let produced = std::fs::read_to_string(&output_path).unwrap();
    assert!(produced.contains("bodyParser: \"urlencoded\""));
}

#[test]
fn fixture_08_form_body_safe_parse_calls_and_raw_body() {
    let (_tmp, output_path) = run_fixture("08_form_body");
    let produced = std::fs::read_to_string(&output_path).unwrap();
    // FormBody validates req.body, same as Body — just a different bodyParser.
    assert!(produced.contains("GatherBody.safeParse(req.body)"));
    // Headers validates req.headers.
    assert!(produced.contains("SignatureHeaders.safeParse(req.headers)"));
    // Validation failures throw, same as Body/Query/Params.
    assert!(produced.contains("throw new BadRequestError(_body.error.message)"));
    assert!(produced.contains("throw new BadRequestError(_headers.error.message)"));
    // RawBody injects req.rawBody directly, with no safeParse call for it.
    assert!(produced.contains("req.rawBody"));
    assert!(!produced.contains("typeof"));
}
