import { anvilUnplugin, type AnvilPluginOptions } from "./index.js";

/** Rollup plugin form of `@anvil-di/anvil-unplugin`. See the package README. */
const rollupPlugin = anvilUnplugin.rollup as (options?: AnvilPluginOptions) => unknown;
export default rollupPlugin;
