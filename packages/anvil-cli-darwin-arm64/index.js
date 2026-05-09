// Resolves the absolute path to the bundled `tsdi` binary. Consumed
// by `tsdi-cli`'s launcher; not meant to be required directly by user
// code (use `tsdi-cli`'s `resolveBinaryPath()` instead).
const path = require("node:path");
exports.binPath = path.join(__dirname, "bin", "anvil");
