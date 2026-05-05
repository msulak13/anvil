import { tsdiUnplugin, type TsdiPluginOptions } from "./index.js";

/** Rspack plugin form of `tsdi-unplugin`. See the package README. */
const rspackPlugin = tsdiUnplugin.rspack as (options?: TsdiPluginOptions) => unknown;
export default rspackPlugin;
