import express from "express";
import { bellowsRoutes } from "@msulak/anvil-bellows";
import { createSpecComponent } from "./spec-component.anvil.js";

const app = express();
const dagger = createSpecComponent();
app.use(bellowsRoutes(dagger.routeDefinitions()));

const port = 3002;
app.listen(port, () => console.log(`Spec server at http://localhost:${port}/openapi.json`));
