import { anvilUnplugin, type AnvilPluginOptions } from "./index.js";

/** Rspack plugin form of `@anvil-di/anvil-unplugin`. See the package README. */
const rspackPlugin = anvilUnplugin.rspack as (options?: AnvilPluginOptions) => unknown;
export default rspackPlugin;
