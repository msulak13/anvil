import express, { type NextFunction, type Request, type Response, Router } from "express";
import cors from "cors";
import { createAppComponent } from "./app-component.anvil.js";
import { NotFoundError } from "./todo-service.js";

const app = express();
app.use(cors());
app.use(express.json());

const dagger = createAppComponent();

const router = Router();
for (const route of dagger.routeDefinitions()) {
  switch (route.method) {
    case "GET":    router.get(route.path, route.handler);    break;
    case "POST":   router.post(route.path, route.handler);   break;
    case "PUT":    router.put(route.path, route.handler);    break;
    case "DELETE": router.delete(route.path, route.handler); break;
    case "PATCH":  router.patch(route.path, route.handler);  break;
  }
}
app.use(router);

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
