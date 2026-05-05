import { tsdiUnplugin, type TsdiPluginOptions } from "./index.js";

/** esbuild plugin form of `tsdi-unplugin`. See the package README. */
const esbuildPlugin = tsdiUnplugin.esbuild as (options?: TsdiPluginOptions) => unknown;
export default esbuildPlugin;
