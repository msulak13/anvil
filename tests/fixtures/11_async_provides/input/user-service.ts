import { Inject } from "tsdi";
import { Database } from "./database";

// Sync @Inject consumer of an async-produced Database. Once the dagger
// has finished its `_resolve` phase, all sync getters return the
// already-awaited values, so this constructor takes Database (not
// Promise<Database>) and never awaits.
@Inject
export class UserService {
  constructor(private db: Database) {}

  list(): string {
    return this.db.query("select * from users");
  }
}
