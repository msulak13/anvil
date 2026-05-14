import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";
import { resolveBinaryPath, unresolvableBinaryError } from "@anvil-di/anvil-bellows-cli";

const execFileAsync = promisify(execFile);

/**
 * A hook that runs before `anvil build`. Consumed by `anvil-unplugin` (M4).
 */
export interface PreBuildHook {
  name: string;
  /** Glob patterns whose changes should trigger a re-run. */
  watchPatterns: string[];
  /** Return true if any of the changed files should trigger this hook. */
  shouldRerun(changedFiles: string[]): boolean;
  /** Execute the hook. */
  run(): Promise<void>;
}

export interface BellowsCodegenOptions {
  /** Directory to scan for `@Controller` files. Defaults to `"./src"`. */
  entry: string;
  /** Output path for the routes module. Defaults to `<entry>/routes.module.anvil.ts`. */
  output?: string;
  /** Output path for the OpenAPI route module. Defaults to `<entry>/schema-route.module.anvil.ts`. */
  openapiOutput?: string;
  /** `info.title` in the generated OpenAPI spec. Defaults to `"API"`. */
  openapiTitle?: string;
  /** `info.version` in the generated OpenAPI spec. Defaults to `"0.0.1"`. */
  openapiVersion?: string;
  /** Path to `tsconfig.json`. */
  tsconfig?: string;
  /** Enable type-checker mode (M3). */
  tsc?: boolean;
}

/**
 * Create a `PreBuildHook` that runs `anvil-bellows` to generate
 * `routes.module.anvil.ts` and `schema-route.module.anvil.ts`.
 *
 * @example
 * ```ts
 * import { bellowsCodegen } from "@anvil-di/anvil-bellows";
 *
 * // In anvil-unplugin options (M4):
 * anvil({ preBuild: [bellowsCodegen({ entry: "src/" })] })
 * ```
 */
export function bellowsCodegen(options: BellowsCodegenOptions): PreBuildHook {
  const entryAbs = path.resolve(options.entry);
  const watchPattern = `${entryAbs.replace(/\\/g, "/")}/**/*.ts`;

  return {
    name: "anvil-bellows-codegen",
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
      if (options.openapiOutput !== undefined) {
        args.push("--openapi-output", options.openapiOutput);
      }
      if (options.openapiTitle !== undefined) {
        args.push("--openapi-title", options.openapiTitle);
      }
      if (options.openapiVersion !== undefined) {
        args.push("--openapi-version", options.openapiVersion);
      }
      if (options.tsconfig !== undefined) {
        args.push("--tsconfig", options.tsconfig);
      }
      if (options.tsc === true) {
        args.push("--tsc");
      }

      const { stdout, stderr } = await execFileAsync(binary, args);
      if (stdout) process.stdout.write(stdout);
      if (stderr) process.stderr.write(stderr);
    },
  };
}
