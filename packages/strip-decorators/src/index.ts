/**
 * `@anvil-di/strip-decorators` — remove anvil's and bellows' no-op decorators
 * from source before a bundler transforms it.
 *
 * anvil's decorators carry no runtime behaviour: every one of them is an
 * identity function, and the wiring they describe is read out of the source
 * by the Rust toolchain during codegen, long before a bundler runs. That
 * makes them safe to delete rather than emit — which matters because oxc
 * (and therefore Rolldown, Vite 6+, and Rspack) implements only the legacy
 * decorator convention, while anvil is standard-decorators-only.
 *
 * Use the plugin if your bundler is supported by unplugin:
 *
 * ```ts
 * // rolldown.config.ts
 * import { stripDecorators } from "@anvil-di/strip-decorators/rollup";
 *
 * export default { plugins: [stripDecorators()] };
 * ```
 *
 * ...or call {@link stripNoopDecorators} directly from a hand-written
 * transform.
 */

export {
  stripNoopDecorators,
  mayContainNoopDecorators,
  type StripOptions,
  type StripResult,
} from "./strip.js";
export { stripDecoratorsUnplugin, type StripDecoratorsPluginOptions } from "./plugin.js";
export { stripDecoratorsUnplugin as default } from "./plugin.js";
