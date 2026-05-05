import { tsdiUnplugin, type TsdiPluginOptions } from "./index.js";

/** Webpack plugin form of `tsdi-unplugin`. See the package README. */
const webpackPlugin = tsdiUnplugin.webpack as (options?: TsdiPluginOptions) => unknown;
export default webpackPlugin;
