import type { JSONSchema7 } from "json-schema";

export type { JSONSchema7 };

export interface Validator<T> {
  safeParse(input: unknown): { success: true; data: T } | { success: false; error: unknown };
  jsonSchema?(): JSONSchema7;
}

export type Body<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;
export type Query<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;
export type Params<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;
export type Responds<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;

export function withJsonSchema<T>(
  validator: Validator<T>,
  schema: JSONSchema7,
): Validator<T> & { jsonSchema(): JSONSchema7 } {
  return {
    safeParse: (input) => validator.safeParse(input),
    jsonSchema: () => schema,
  };
}
