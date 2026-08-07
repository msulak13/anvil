import { parseSync } from "oxc-parser";
import MagicString from "magic-string";
import { createUnplugin, type UnpluginInstance } from "unplugin";

/**
 * Modules whose every export is a decorator with no runtime behaviour.
 *
 * Both anvil packages qualify by construction: their decorators are stubs
 * that `return target`, and all semantics are read out of the source by the
 * Rust toolchain during codegen. This is invariant #1 of the project —
 * "No runtime reflection. Decorators are no-ops at runtime."
 */
const DEFAULT_NOOP_MODULES: readonly RegExp[] = [/^@anvil-di\/(anvil|bellows)$/];

/**
 * A decorator-shaped token: `@Name` at a position where one can legally
 * appear. Used only by {@link mayContainNoopDecorators}; a JSDoc `@param`
 * can match, which costs a parse and nothing else.
 */
const DECORATOR_TOKEN_RE = /(?:^|[\s{;(])@[A-Za-z_$][\w$]*/;

/** Options for {@link stripNoopDecorators}. */
export interface StripOptions {
  /**
   * Additional modules whose decorators are known to be no-ops, beyond
   * `@anvil-di/anvil` and `@anvil-di/bellows`.
   *
   * A string matches a module specifier exactly; a RegExp is tested against
   * it. Use this for a project's own marker decorators — a `@Public(reason)`
   * that exists only so `anvil-bellows` can read it at codegen time, for
   * instance. Relative specifiers are matched as written in the importing
   * file, so prefer a RegExp: `/(?:^|\/)http\/public-route(?:\.js)?$/`.
   *
   * Adding a module here asserts that none of its decorators do anything at
   * runtime. That claim is not checked; get it wrong and behaviour is
   * silently dropped.
   */
  additionalModules?: readonly (string | RegExp)[];
}

/** The result of a successful strip. */
export interface StripResult {
  /** Source with the no-op decorators removed. */
  code: string;
  /** Source map for the removals, as returned by magic-string. */
  map: ReturnType<MagicString["generateMap"]>;
  /** How many decorators were removed. */
  removed: number;
}

interface EstreeNode {
  type?: string;
  [key: string]: unknown;
}

function matches(specifier: string, patterns: readonly (string | RegExp)[]): boolean {
  return patterns.some((p) => (typeof p === "string" ? p === specifier : p.test(specifier)));
}

/**
 * Local names introduced by imports from a no-op decorator module, honouring
 * aliases (`import { Inject as I }`). Namespace imports are tracked apart so
 * `@anvil.Inject` resolves through its object rather than its property.
 */
function noopBindings(
  program: EstreeNode,
  patterns: readonly (string | RegExp)[],
): { named: Set<string>; namespaces: Set<string> } {
  const named = new Set<string>();
  const namespaces = new Set<string>();
  for (const node of (program["body"] as EstreeNode[] | undefined) ?? []) {
    if (node.type !== "ImportDeclaration") continue;
    // `import type { ... }` contributes no runtime binding, so it can never
    // be the callee of a decorator.
    if (node["importKind"] === "type") continue;
    const source = node["source"] as { value?: string } | undefined;
    if (source?.value === undefined || !matches(source.value, patterns)) continue;
    for (const spec of (node["specifiers"] as EstreeNode[] | undefined) ?? []) {
      if (spec["importKind"] === "type") continue;
      const local = (spec["local"] as { name?: string } | undefined)?.name;
      if (local === undefined) continue;
      if (spec.type === "ImportNamespaceSpecifier") namespaces.add(local);
      else named.add(local);
    }
  }
  return { named, namespaces };
}

/** The identifier a decorator dispatches on: `@A`, `@A(x)`, `@ns.A`, `@ns.A(x)`. */
function decoratorRoot(
  expression: EstreeNode,
): { kind: "named" | "member"; name: string } | null {
  let expr = expression;
  if (expr.type === "CallExpression") expr = expr["callee"] as EstreeNode;
  if (expr.type === "Identifier") return { kind: "named", name: expr["name"] as string };
  if (expr.type === "MemberExpression" && expr["computed"] !== true) {
    const object = expr["object"] as EstreeNode;
    if (object.type === "Identifier") return { kind: "member", name: object["name"] as string };
  }
  return null;
}

function walk(node: unknown, visit: (node: EstreeNode) => void): void {
  if (node === null || typeof node !== "object") return;
  if (Array.isArray(node)) {
    for (const child of node) walk(child, visit);
    return;
  }
  const candidate = node as EstreeNode;
  if (typeof candidate.type === "string") visit(candidate);
  for (const key of Object.keys(candidate)) {
    if (key === "type") continue;
    walk(candidate[key], visit);
  }
}

/**
 * Delete every decorator that resolves to a no-op decorator module.
 *
 * Returns `null` when there is nothing to do — no such import, or no such
 * decorator — so callers can hand the original source and its existing
 * source map straight through.
 *
 * ## Why deleting is correct
 *
 * anvil's and bellows' decorators are identity functions: `@Provides`,
 * `@Controller`, `@Get` and the rest all `return target`. They exist so user
 * code typechecks and so the Rust toolchain can read the wiring out of the
 * source during `anvil build` / `anvil-bellows`, which happens before any
 * bundler runs. Nothing observable happens when they are applied, so
 * removing them produces the same program without the
 * `__esDecorate`/`__runInitializers` machinery a real emit would inline.
 *
 * ## Why it is needed
 *
 * oxc's decorator transform implements only the legacy
 * (`experimentalDecorators`) convention, which anvil forbids. Left alone,
 * standard decorators pass through a bundle verbatim and no JS engine can
 * parse them; transformed as legacy, method decorators are called with the
 * wrong signature and corrupt the class. Routing the affected files through
 * `tsc` sidesteps both, at the cost of a dependency on TypeScript's JS
 * compiler API — which TypeScript 7 does not ship.
 *
 * ## Safety
 *
 * Only decorators whose binding comes from a listed module are removed.
 * Anything else is left exactly where it is, so a decorator with real
 * behaviour survives into the output and fails loudly rather than being
 * silently dropped.
 *
 * @param code source text of a `.ts` / `.tsx` file
 * @param id file path, used for parser mode selection and diagnostics
 */
export function stripNoopDecorators(
  code: string,
  id: string,
  options: StripOptions = {},
): StripResult | null {
  const patterns: readonly (string | RegExp)[] = [
    ...DEFAULT_NOOP_MODULES,
    ...(options.additionalModules ?? []),
  ];

  const parsed = parseSync(id, code, { lang: id.endsWith(".tsx") ? "tsx" : "ts" });
  if (parsed.errors.length > 0) {
    throw new Error(`strip-decorators: cannot parse ${id}: ${parsed.errors[0]?.message ?? ""}`);
  }

  const program = parsed.program as unknown as EstreeNode;
  const { named, namespaces } = noopBindings(program, patterns);
  if (named.size === 0 && namespaces.size === 0) return null;

  const ranges: Array<[number, number]> = [];
  walk(program, (node) => {
    if (node.type !== "Decorator") return;
    const root = decoratorRoot(node["expression"] as EstreeNode);
    if (root === null) return;
    const owned = root.kind === "named" ? named.has(root.name) : namespaces.has(root.name);
    if (owned) ranges.push([node["start"] as number, node["end"] as number]);
  });
  if (ranges.length === 0) return null;

  const s = new MagicString(code);
  for (const [start, end] of ranges) s.remove(start, end);
  return {
    code: s.toString(),
    map: s.generateMap({ source: id, hires: true }),
    removed: ranges.length,
  };
}

/**
 * Cheap pre-filter, so a bundler can skip parsing files that cannot possibly
 * contain a strippable decorator.
 *
 * Conservative by construction: never a false negative, and a false positive
 * costs only a wasted parse. A `RegExp` in `additionalModules` is written to
 * match a module specifier, so it cannot be tested against whole source text
 * — any file with decorator syntax counts as a candidate when one is set.
 */
export function mayContainNoopDecorators(code: string, options: StripOptions = {}): boolean {
  if (!DECORATOR_TOKEN_RE.test(code)) return false;
  if (code.includes("@anvil-di/anvil") || code.includes("@anvil-di/bellows")) return true;
  for (const pattern of options.additionalModules ?? []) {
    if (typeof pattern === "string") {
      if (code.includes(pattern)) return true;
    } else {
      // A specifier pattern cannot be matched against whole source text.
      return true;
    }
  }
  return false;
}

/** Options for the standalone decorator-strip plugin. */
export interface StripDecoratorsPluginOptions extends StripOptions {
  /**
   * File extensions the transform applies to. Defaults to `.ts` and `.tsx`.
   */
  extensions?: readonly string[];
}

const DEFAULT_EXTENSIONS = [".ts", ".tsx"] as const;

/**
 * The decorator strip on its own, without anvil's codegen hooks — for a
 * project that already runs `anvil build` / `anvil-bellows` its own way
 * (a `gen` npm script, say) and only needs its bundle to come out
 * runnable.
 *
 * ```ts
 * // rolldown.config.ts
 * import { stripDecoratorsUnplugin } from "@anvil-di/anvil-unplugin/strip";
 *
 * export default { plugins: [stripDecoratorsUnplugin.rolldown()] };
 * ```
 *
 * Projects using the main `anvilUnplugin` get this already; see its
 * `stripDecorators` option.
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
      const file = id.split("?")[0] ?? id;
      return extensions.some((ext) => file.endsWith(ext));
    },
    transform(code: string, id: string) {
      if (!mayContainNoopDecorators(code, stripOptions)) return null;
      const result = stripNoopDecorators(code, id.split("?")[0] ?? id, stripOptions);
      if (result === null) return null;
      // Serialized: magic-string types `sourcesContent` as
      // `(string | null)[]`, which rollup's map type rejects.
      return { code: result.code, map: result.map.toString() };
    },
  };
});
