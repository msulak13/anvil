// A pure TypeScript interface — compiles away at runtime, so the
// generated factory must alias it to a concrete class via @Binds. The
// IR identifies the interface by `(module path, exported name)`,
// exactly the same way it identifies a class — type identity does not
// require running tsc.
export interface Heater {
  heat(): void;
}
