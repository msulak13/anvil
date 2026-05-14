/* @ts-self-types="./anvil_codegen_wasm.d.ts" */
import * as wasm from "./anvil_codegen_wasm_bg.wasm";
import { __wbg_set_wasm } from "./anvil_codegen_wasm_bg.js";

__wbg_set_wasm(wasm);

export {
    compile
} from "./anvil_codegen_wasm_bg.js";
