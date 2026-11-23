/**
 * A deferred handle to a binding. Inject `Lazy<T>` instead of `T` to delay
 * constructing `T` until the first {@link Lazy.get} call, or to break a
 * dependency cycle. The first result is memoized per handle.
 *
 * The generated dagger constructs these; user code only calls `get()`. An
 * async-provided `T` is already resolved by `createX()`, so `Lazy` defers
 * only synchronous construction.
 */
export class Lazy<T> {
  #factory: (() => T) | undefined;
  #value: T | undefined;

  constructor(factory: () => T) {
    this.#factory = factory;
  }

  get(): T {
    if (this.#factory) {
      this.#value = this.#factory();
      this.#factory = undefined;
    }
    return this.#value as T;
  }
}
