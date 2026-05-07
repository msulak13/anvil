/**
 * `tsdi-unplugin` — runs tsdi's Rust codegen as part of any bundler's
 * pipeline. One adapter; every bundler (Vite, Rollup, Webpack, Rspack,
 * esbuild) via [unplugin](https://github.com/unjs/unplugin).
 *
 * The plugin invokes the `tsdi` CLI binary on:
 *   - `buildStart` (once at the beginning of every build)
 *   - `watchChange` (debounced, when any source `.ts` is edited in dev)
 *
 * It emits the generated `*.tsdi.ts` files alongside their source
 * components — same shape as running `tsdi build` manually. The
 * bundler's normal TS pipeline picks them up.
 *
 * @example
 * ```ts
 * // vite.config.ts
 * import tsdi from "tsdi-unplugin/vite";
 *
 * export default defineConfig({
 *   plugins: [tsdi()],
 * });
 * ```
 */

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";
import {
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
  existsSync,
  mkdirSync,
} from "node:fs";
import { createUnplugin, type UnpluginInstance } from "unplugin";
import { resolveBinaryPath, unresolvableBinaryError } from "tsdi-cli";
import { compile as wasmCompile } from "tsdi-codegen-wasm";

const execFileAsync = promisify(execFile);

/** Plugin options. Every field is optional; sensible defaults match the CLI. */
export interface TsdiPluginOptions {
  /**
   * Component entry files to (re)generate. Each path is passed to
   * `tsdi build --entry <path>`. Defaults to whatever `tsdi.config.json`
   * (auto-discovered from the project root) declares — the same logic
   * `tsdi build` uses without arguments.
   */
  entries?: string[];
  /**
   * Path to a `tsconfig.json`. Forwarded to the CLI as `--tsconfig`.
   * Lets the resolver honor `paths` / `baseUrl` aliases.
   */
  tsconfig?: string;
  /**
   * Path to the `tsdi` binary. When unset, the plugin resolves the
   * native binary via the `tsdi-cli` npm launcher — which in turn
   * picks the right per-platform package (`tsdi-cli-linux-x64`,
   * `tsdi-cli-darwin-arm64`, etc.) installed alongside it.
   *
   * Override only when you need to point at a custom build (e.g. a
   * `cargo build`-produced binary outside `node_modules`).
   */
  cli?: string;
  /**
   * Debounce window (ms) for re-running codegen after a file change in
   * watch mode. Default: 100 — same value `tsdi watch` uses for its
   * own filesystem-event debouncing.
   */
  debounceMs?: number;
  /**
   * Glob patterns the plugin will watch in dev mode. Editing any
   * matching file triggers a debounced rebuild. Defaults to `**\/*.ts`
   * — the codegen's M2 resolver is the layer that decides which files
   * actually feed into a particular component, so over-watching is
   * harmless (extra files just get filtered out by the resolver).
   */
  include?: string[];
  /**
   * Which codegen backend to use (M13).
   *
   * - `"native"` (default): spawn the `tsdi` Rust binary via
   *   `tsdi-cli`. Lower steady-state codegen latency, requires the
   *   per-platform binary to be installed.
   * - `"wasm"`: run the entire pipeline in-process through
   *   `tsdi-codegen-wasm`. No `spawnSync` cost in the bundler hot
   *   path, works anywhere V8 runs (Bun, Cloudflare Workers,
   *   StackBlitz, etc.). The WASM compile + instantiate happens once
   *   on first build (~80-150ms); subsequent builds are direct
   *   function calls.
   *
   * The two paths produce byte-identical output for the same input
   * — same parser, same graph builder, same emitter, just sourced
   * from a process spawn vs an in-memory file map.
   */
  mode?: "native" | "wasm";
  /**
   * For `mode: "wasm"`, the directory whose `.ts` / `.tsx` files
   * should be loaded into the in-memory file map. Defaults to the
   * directory containing the first `entries[0]`'s parent folder.
   * The WASM resolver walks transitively from each entry — extra
   * files in the map are harmless, missing files surface as a
   * diagnostic. Set this when your sources live somewhere other
   * than the entry's own subtree.
   */
  rootDir?: string;
}

/**
 * The unplugin factory. Each per-bundler entry point (`/vite`,
 * `/rollup`, etc.) just calls `tsdiUnplugin.<name>(options)` to get a
 * bundler-specific plugin object.
 */
export const tsdiUnplugin: UnpluginInstance<TsdiPluginOptions | undefined, false> =
  createUnplugin<TsdiPluginOptions | undefined>((rawOptions) => {
    // Default `cli` to whatever `tsdi-cli` resolves at runtime (the
    // matching `tsdi-cli-<platform>-<arch>` package, or the
    // `TSDI_CLI_BIN` env override). This is the recommended path for
    // npm-distributed users; explicit `cli:` overrides take priority
    // for monorepo dev where the binary lives in `target/release`.
    const resolvedCli = rawOptions?.cli ?? resolveBinaryPath();
    const mode: "native" | "wasm" = rawOptions?.mode ?? "native";
    const options: Required<
      Omit<TsdiPluginOptions, "entries" | "tsconfig" | "cli" | "rootDir">
    > &
      Pick<TsdiPluginOptions, "entries" | "tsconfig" | "rootDir"> & {
        cli: string | null;
      } = {
      cli: resolvedCli,
      debounceMs: rawOptions?.debounceMs ?? 100,
      include: rawOptions?.include ?? ["**/*.ts"],
      mode,
      entries: rawOptions?.entries,
      tsconfig: rawOptions?.tsconfig,
      rootDir: rawOptions?.rootDir,
    };

    let pending: NodeJS.Timeout | undefined;
    let inFlight: Promise<void> | undefined;

    async function runCodegenOnce(): Promise<void> {
      // Coalesce concurrent invocations: the second caller awaits the
      // first's in-flight Promise rather than spawning a duplicate
      // `tsdi build`.
      if (inFlight !== undefined) {
        return inFlight;
      }
      inFlight = (async () => {
        try {
          if (options.mode === "wasm") {
            await runWasmBuild(options);
          } else {
            await invokeBuild(options);
          }
        } finally {
          inFlight = undefined;
        }
      })();
      return inFlight;
    }

    function scheduleRebuild(): void {
      if (pending !== undefined) {
        clearTimeout(pending);
      }
      pending = setTimeout(() => {
        pending = undefined;
        // Errors in watch mode are reported by `tsdi build` itself
        // (writing to stderr); we don't crash the dev server.
        void runCodegenOnce().catch(() => {});
      }, options.debounceMs);
    }

    return {
      name: "tsdi-unplugin",
      // unplugin's universal hooks. Each bundler maps these onto its
      // own lifecycle (Vite's `buildStart`, Webpack's tap, etc.).
      async buildStart() {
        // Bundler may abort early if codegen fails — the bundler's
        // own error-reporting plumbing surfaces the diagnostic.
        await runCodegenOnce();
      },
      watchChange(id: string) {
        // Skip our own output so we don't infinite-loop.
        if (id.endsWith(".tsdi.ts")) {
          return;
        }
        if (!id.endsWith(".ts") && !id.endsWith(".tsx")) {
          return;
        }
        scheduleRebuild();
      },
    };
  });

async function invokeBuild(options: {
  cli: string | null;
  entries?: string[];
  tsconfig?: string;
}): Promise<void> {
  if (options.cli === null) {
    throw new Error(unresolvableBinaryError());
  }
  const cli: string = options.cli;
  // The CLI accepts a single --entry per invocation. When the user
  // supplies multiple, we spawn once per entry — concurrency is fine
  // because each entry's output file is co-located with that entry,
  // so writes don't race.
  const tsconfigArgs = options.tsconfig === undefined ? [] : ["--tsconfig", options.tsconfig];
  if (options.entries === undefined || options.entries.length === 0) {
    // Let the CLI auto-discover from `tsdi.config.json` /
    // `package.json#tsdi`.
    await execFileAsync(cli, ["build", ...tsconfigArgs]);
    return;
  }
  await Promise.all(
    options.entries.map((entry) =>
      execFileAsync(cli, [
        "build",
        "--entry",
        path.resolve(entry),
        ...tsconfigArgs,
      ]),
    ),
  );
}

/**
 * M13: in-process codegen via `tsdi-codegen-wasm`.
 *
 * Reads every `.ts` / `.tsx` file under `rootDir` (defaulting to the
 * first entry's parent dir) into an in-memory file map, hands it to
 * the WASM compile function, and writes the emitted dagger files
 * back to disk co-located with their components — same layout the
 * native CLI produces, just without the `spawnSync` round-trip.
 *
 * Diagnostics are written to stderr in the same format as `tsdi
 * check`. Future revisions of this hook will route them through the
 * unplugin context's `this.error` / `this.warn` for inline overlay
 * support, but those APIs differ across bundlers and stderr is the
 * lowest-common-denominator surface that works everywhere today.
 */
async function runWasmBuild(options: {
  entries?: string[];
  tsconfig?: string;
  rootDir?: string;
}): Promise<void> {
  if (options.entries === undefined || options.entries.length === 0) {
    throw new Error(
      "tsdi-unplugin: `mode: \"wasm\"` requires explicit `entries:` paths " +
        "(auto-discovery from tsdi.config.json isn't wired through the WASM " +
        "build yet). Specify `entries: ['src/**/*-component.ts']` or similar.",
    );
  }
  const root =
    options.rootDir ?? path.dirname(path.resolve(options.entries[0]!));
  const files = collectSourceFiles(root);

  // Also pull in the `tsdi` runtime stub so bare-specifier imports
  // resolve. The stubs live at `node_modules/tsdi/dist/index.d.ts`
  // for the published runtime, or `packages/tsdi/dist/index.d.ts`
  // in the monorepo. Locate via require.resolve so we don't hardcode
  // either path.
  try {
    const tsdiPkg = require.resolve("tsdi/package.json");
    const tsdiDir = path.dirname(tsdiPkg);
    addRecursive(tsdiDir, files);
  } catch {
    // The `tsdi` runtime isn't installed — caller probably stubbed it
    // themselves under a different name; the WASM resolver will
    // surface a `BareNotFound` if anything's actually missing.
  }

  const aliases = options.tsconfig === undefined ? [] : tsconfigPathsFrom(options.tsconfig);

  for (const entry of options.entries) {
    const entryAbs = path.resolve(entry);
    const result = wasmCompile({
      entryPath: entryAbs,
      files,
      aliases,
      version: "0.0.1-wasm",
    });
    for (const diag of result.diagnostics) {
      process.stderr.write(`tsdi: ${diag.code}: ${diag.summary}\n`);
      process.stderr.write(`  at ${diag.primary.path}:${diag.primary.start}\n`);
    }
    for (const emitted of result.emittedFiles) {
      const dir = path.dirname(emitted.path);
      if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
      writeFileSync(emitted.path, emitted.contents, "utf8");
    }
  }
}

/**
 * Read every `.ts` / `.tsx` file under `root` into a map keyed by
 * absolute path. Skips `node_modules` (those entries are added
 * separately when the host's `tsdi` runtime stub is located).
 */
function collectSourceFiles(root: string): Record<string, string> {
  const out: Record<string, string> = {};
  addRecursive(root, out);
  return out;
}

function addRecursive(dir: string, out: Record<string, string>): void {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (entry.name === "node_modules" || entry.name.startsWith(".")) continue;
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      addRecursive(abs, out);
      continue;
    }
    if (
      entry.isFile() &&
      (abs.endsWith(".ts") || abs.endsWith(".tsx") || abs.endsWith(".d.ts"))
    ) {
      try {
        out[abs] = readFileSync(abs, "utf8");
      } catch {
        // Skip unreadable files; the resolver will surface them if they
        // were actually needed.
      }
    }
  }
  // Avoid lint-warn for unused statSync; the import is documented as
  // available for future host-specific filesystem probes.
  void statSync;
}

/**
 * Pull `compilerOptions.paths` aliases out of a tsconfig.json on
 * disk. Best-effort — bad JSON or missing paths field returns an
 * empty list rather than throwing.
 */
function tsconfigPathsFrom(tsconfigPath: string): {
  pattern: string;
  target: string;
  baseDir: string;
}[] {
  try {
    const raw = readFileSync(tsconfigPath, "utf8");
    // Strip simple block comments — tsconfig.json technically
    // permits them. Not a full JSONC parser; covers the 99% case.
    const cleaned = raw.replace(/\/\*[\s\S]*?\*\//g, "");
    const cfg = JSON.parse(cleaned) as {
      compilerOptions?: {
        baseUrl?: string;
        paths?: Record<string, string[]>;
      };
    };
    const baseUrl = cfg.compilerOptions?.baseUrl ?? ".";
    const baseDir = path.resolve(path.dirname(tsconfigPath), baseUrl);
    const paths = cfg.compilerOptions?.paths ?? {};
    return Object.entries(paths).flatMap(([pattern, targets]) =>
      targets.length === 0
        ? []
        : [{ pattern, target: targets[0]!, baseDir }],
    );
  } catch {
    return [];
  }
}

export default tsdiUnplugin;
