import { tsdiUnplugin, type TsdiPluginOptions } from "./index.js";

/** Rollup plugin form of `tsdi-unplugin`. See the package README. */
const rollupPlugin = tsdiUnplugin.rollup as (options?: TsdiPluginOptions) => unknown;
export default rollupPlugin;
