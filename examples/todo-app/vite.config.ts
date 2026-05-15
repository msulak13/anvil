import { defineConfig } from "vite";
import path from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import anvil from "@anvil-di/anvil-unplugin/vite";
import { bellowsCodegen } from "@anvil-di/anvil-bellows";

// In the monorepo the darwin-arm64 platform package isn't committed, so
// fall back to the cargo debug build when the env vars aren't already set.
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const ext = process.platform === "win32" ? ".exe" : "";
process.env.ANVIL_CLI_BIN ??= path.join(repoRoot, "target", "debug", `anvil${ext}`);
process.env.ANVIL_BELLOWS_CLI_BIN ??= path.join(repoRoot, "target", "debug", `anvil-bellows${ext}`);

export default defineConfig({
  plugins: [
    react(),
    anvil({
      // Regenerate server/src/app-component.anvil.ts on every build / file save.
      entries: ["server/src/app-component.ts", "server/src/spec-component.ts"],
      preBuild: [
        // Generate routes.module.anvil.ts + schema-route.module.anvil.ts from @Controller decorators.
        bellowsCodegen({ entry: "server/src", openapiTitle: "Todo API" }),
      ],
    }),
  ],
  server: {
    port: 5173,
  },
});
