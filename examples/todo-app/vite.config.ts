import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import anvil from "@anvil-di/anvil-unplugin/vite";
import { bellowsCodegen } from "@anvil-di/anvil-bellows";

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
