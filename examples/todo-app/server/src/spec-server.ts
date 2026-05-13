import express from "express";
import { createSpecComponent } from "./spec-component.anvil.js";

const app = express();
const dagger = createSpecComponent();

for (const route of dagger.routeDefinitions()) {
  app.get(route.path, route.handler);
}

const port = 3002;
app.listen(port, () => console.log(`Spec server at http://localhost:${port}/openapi.json`));
