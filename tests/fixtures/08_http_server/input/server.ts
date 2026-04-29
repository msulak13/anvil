// Real entry point — *not* part of the @Component graph (nothing in
// the dagger imports this file). The dagger is constructed once at
// startup, and request handlers reach into it through the entry-point
// methods on the abstract class.
import { createServer, IncomingMessage, ServerResponse } from "node:http";
import { createAppComponent } from "./app-component.tsdi";

const app = createAppComponent();
const log = app.logger();
const users = app.userService();

const server = createServer((req: IncomingMessage, res: ServerResponse) => {
  const url = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);
  log.info(`${req.method ?? "?"} ${url.pathname}`);

  if (req.method === "GET" && url.pathname === "/users") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(users.list()));
    return;
  }

  const idMatch = url.pathname.match(/^\/users\/(\d+)$/);
  if (req.method === "GET" && idMatch) {
    const user = users.byId(Number(idMatch[1]));
    if (user === undefined) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(user));
    return;
  }

  res.writeHead(404).end();
});

const port = Number(process.env["PORT"] ?? 3000);
server.listen(port, () => log.info(`listening on :${port}`));
