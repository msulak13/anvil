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
import { createUnplugin, type UnpluginInstance } from "unplugin";

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
   * Path to (or name of) the `tsdi` binary. Defaults to `"tsdi"`,
   * which expects it on `$PATH` or in `node_modules/.bin/`. Override
   * when running from a workspace root that hasn't yet installed the
   * binary into `node_modules`.
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
}

/**
 * The unplugin factory. Each per-bundler entry point (`/vite`,
 * `/rollup`, etc.) just calls `tsdiUnplugin.<name>(options)` to get a
 * bundler-specific plugin object.
 */
export const tsdiUnplugin: UnpluginInstance<TsdiPluginOptions | undefined, false> =
  createUnplugin<TsdiPluginOptions | undefined>((rawOptions) => {
    const options: Required<Omit<TsdiPluginOptions, "entries" | "tsconfig">> &
      Pick<TsdiPluginOptions, "entries" | "tsconfig"> = {
      cli: rawOptions?.cli ?? "tsdi",
      debounceMs: rawOptions?.debounceMs ?? 100,
      include: rawOptions?.include ?? ["**/*.ts"],
      entries: rawOptions?.entries,
      tsconfig: rawOptions?.tsconfig,
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
          await invokeBuild(options);
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
  cli: string;
  entries?: string[];
  tsconfig?: string;
}): Promise<void> {
  // The CLI accepts a single --entry per invocation. When the user
  // supplies multiple, we spawn once per entry — concurrency is fine
  // because each entry's output file is co-located with that entry,
  // so writes don't race.
  const tsconfigArgs = options.tsconfig === undefined ? [] : ["--tsconfig", options.tsconfig];
  if (options.entries === undefined || options.entries.length === 0) {
    // Let the CLI auto-discover from `tsdi.config.json` /
    // `package.json#tsdi`.
    await execFileAsync(options.cli, ["build", ...tsconfigArgs]);
    return;
  }
  await Promise.all(
    options.entries.map((entry) =>
      execFileAsync(options.cli, [
        "build",
        "--entry",
        path.resolve(entry),
        ...tsconfigArgs,
      ]),
    ),
  );
}

export default tsdiUnplugin;
