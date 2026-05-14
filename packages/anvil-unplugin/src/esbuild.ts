import { anvilUnplugin, type AnvilPluginOptions } from "./index.js";

/** esbuild plugin form of `@anvil-di/anvil-unplugin`. See the package README. */
const esbuildPlugin = anvilUnplugin.esbuild as (options?: AnvilPluginOptions) => unknown;
export default esbuildPlugin;
