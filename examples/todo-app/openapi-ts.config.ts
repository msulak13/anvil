import { defineConfig } from "@hey-api/openapi-ts";

export default defineConfig({
  input: "http://localhost:3002/openapi.json",
  output: {
    path: "src/api",
    format: "prettier",
  },
  plugins: [
    "@hey-api/client-fetch",
    "@tanstack/react-query",
  ],
});
