/* tslint:disable */
/* eslint-disable */

/**
 * `wasm-bindgen` entry point. Accepts a JS object matching
 * [`CompileInput`], returns a JS object matching [`CompileOutput`].
 * Errors thrown into JS land map to the host's normal exception
 * handling — `@msulak/anvil-unplugin` translates these into bundler-side
 * diagnostic surfaces.
 */
export function compile(input_js: any): any;
