import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import anvil from "@msulak/anvil-unplugin/vite";
import { bellowsCodegen } from "@msulak/anvil-bellows";
import { bellowsOpenApi } from "@msulak/anvil-bellows-openapi";

export default defineConfig({
  plugins: [
    react(),
    anvil({
      // Regenerate server/src/app-component.anvil.ts on every build / file save.
      entries: ["server/src/app-component.ts"],
      preBuild: [
        // Generate server/src/routes.module.ts from @Controller decorators.
        bellowsCodegen({ entry: "server/src" }),
      ],
      postBuild: [
        // Generate openapi.json from the updated routes.
        bellowsOpenApi({
          entry: "server/src",
          output: "openapi.json",
          config: "openapi.config.json",
        }),
      ],
    }),
  ],
  server: {
    port: 5173,
  },
});
