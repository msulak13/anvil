import express, { type NextFunction, type Request, type Response } from "express";
import cors from "cors";
import { bellowsRoutes } from "@anvil-di/anvil-bellows";
import { createAppComponent } from "./app-component.anvil.js";
import { NotFoundError } from "./todo-service.js";

const app = express();
app.use(cors());
app.use(express.json());

const dagger = createAppComponent();
app.use(bellowsRoutes(dagger.routeDefinitions()));

app.use((err: unknown, _req: Request, res: Response, _next: NextFunction) => {
  if (err instanceof NotFoundError) {
    res.status(404).json({ error: err.message });
    return;
  }
  const message = err instanceof Error ? err.message : "Internal server error";
  res.status(500).json({ error: message });
});

const port = 3001;
app.listen(port, () => console.log(`Todo API at http://localhost:${port}`));
