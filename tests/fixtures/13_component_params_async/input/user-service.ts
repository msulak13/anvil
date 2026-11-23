import { Inject } from "@anvil-di/anvil";
import { Database } from "./database";
import { Env } from "./env";

@Inject
export class UserService {
  constructor(
    private db: Database,
    private env: Env,
  ) {}

  describe(): string {
    return `${this.db.url} (${this.env.databaseUrl})`;
  }
}
