import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";
import {
  resolveBinaryPath,
  unresolvableBinaryError,
} from "@msulak/anvil-bellows-openapi-cli";

const execFileAsync = promisify(execFile);

/**
 * A hook that runs after `anvil build`. Consumed by `anvil-unplugin` (M4).
 */
export interface PostBuildHook {
  name: string;
  /** Glob patterns whose changes should trigger a re-run. */
  watchPatterns: string[];
  /** Return true if any of the changed files should trigger this hook. */
  shouldRerun(changedFiles: string[]): boolean;
  /** Execute the hook. */
  run(): Promise<void>;
}

export interface BellowsOpenApiOptions {
  /** Directory to scan for `@Controller` files. Defaults to `"./src"`. */
  entry: string;
  /** Output file path. Defaults to `<entry>/openapi.json`. */
  output?: string;
  /** Output format: `"json"` or `"yaml"`. Defaults to `"json"`. */
  format?: "json" | "yaml";
  /** Path to a JSON config file (info, servers, securitySchemes). */
  config?: string;
}

/**
 * Create a `PostBuildHook` that runs `anvil-bellows-openapi` to generate an
 * OpenAPI 3.1 document from `@Controller` source files.
 *
 * @example
 * ```ts
 * import { bellowsCodegen } from "@msulak/anvil-bellows";
 * import { bellowsOpenApi } from "@msulak/anvil-bellows-openapi";
 *
 * anvil({
 *   preBuild: [bellowsCodegen({ entry: "src/" })],
 *   postBuild: [bellowsOpenApi({ entry: "src/", config: "openapi.config.json" })],
 * })
 * ```
 */
export function bellowsOpenApi(options: BellowsOpenApiOptions): PostBuildHook {
  const entryAbs = path.resolve(options.entry);
  const watchPattern = `${entryAbs.replace(/\\/g, "/")}/**/*.ts`;

  return {
    name: "anvil-bellows-openapi",
    watchPatterns: [watchPattern],

    shouldRerun(changedFiles: string[]): boolean {
      return changedFiles.some(
        (f) =>
          f.startsWith(entryAbs) &&
          f.endsWith(".ts") &&
          !f.endsWith(".d.ts"),
      );
    },

    async run(): Promise<void> {
      const binary = resolveBinaryPath();
      if (binary === null) {
        throw new Error(unresolvableBinaryError());
      }

      const args: string[] = ["--entry", options.entry];
      if (options.output !== undefined) {
        args.push("--output", options.output);
      }
      if (options.format !== undefined) {
        args.push("--format", options.format);
      }
      if (options.config !== undefined) {
        args.push("--config", options.config);
      }

      const { stdout, stderr } = await execFileAsync(binary, args);
      if (stdout) process.stdout.write(stdout);
      if (stderr) process.stderr.write(stderr);
    },
  };
}
