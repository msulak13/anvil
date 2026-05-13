import express from "express";
import { createAppComponent } from "./app-component.anvil.js";

const app = express();
const dagger = createAppComponent();

for (const route of dagger.routeDefinitions()) {
  if (route.method === "GET" && route.path === "/openapi.json") {
    app.get(route.path, route.handler);
    break;
  }
}

const port = 3002;
app.listen(port, () => console.log(`Spec server at http://localhost:${port}/openapi.json`));
