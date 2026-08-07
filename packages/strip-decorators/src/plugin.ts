import { createUnplugin, type UnpluginInstance } from "unplugin";
import { mayContainNoopDecorators, stripNoopDecorators, type StripOptions } from "./strip.js";

/** Options for the strip-decorators bundler plugin. */
export interface StripDecoratorsPluginOptions extends StripOptions {
  /**
   * File extensions the transform applies to. Defaults to `.ts` and `.tsx`.
   *
   * Decorators only reach a bundler from TypeScript sources in an anvil
   * project, but a project that runs its own pre-transform may need to widen
   * this.
   */
  extensions?: readonly string[];
}

const DEFAULT_EXTENSIONS = [".ts", ".tsx"] as const;

/**
 * unplugin factory. One adapter, every bundler unplugin supports (Vite,
 * Rollup, Rolldown, Webpack, Rspack, esbuild).
 *
 * The transform runs on source, before the bundler's own TypeScript
 * handling, and removes only decorators that resolve to a known no-op
 * module. Files without such a decorator are passed through untouched, so
 * their existing source maps are preserved exactly.
 */
export const stripDecoratorsUnplugin: UnpluginInstance<
  StripDecoratorsPluginOptions | undefined,
  false
> = createUnplugin<StripDecoratorsPluginOptions | undefined>((rawOptions) => {
  const extensions = rawOptions?.extensions ?? DEFAULT_EXTENSIONS;
  const stripOptions: StripOptions =
    rawOptions?.additionalModules === undefined
      ? {}
      : { additionalModules: rawOptions.additionalModules };

  return {
    name: "anvil-strip-decorators",
    // `id` carries a query suffix in some bundlers (`?used`, `?vue&type=...`);
    // compare against the path portion only.
    transformInclude(id: string) {
      const path = id.split("?")[0] ?? id;
      return extensions.some((ext) => path.endsWith(ext));
    },
    transform(code: string, id: string) {
      if (!mayContainNoopDecorators(code, stripOptions)) return null;
      const result = stripNoopDecorators(code, id.split("?")[0] ?? id, stripOptions);
      if (result === null) return null;
      // Serialized rather than passed as an object: magic-string types
      // `sourcesContent` as `(string | null)[]`, which is not assignable to
      // the `string[]` rollup's `ExistingRawSourceMap` wants. Every bundler
      // accepts the JSON form.
      return { code: result.code, map: result.map.toString() };
    },
  };
});

export default stripDecoratorsUnplugin;
