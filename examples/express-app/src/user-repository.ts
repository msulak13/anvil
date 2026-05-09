import { Inject, Singleton } from "@msulak/anvil";

export interface User {
  id: number;
  name: string;
  email: string;
}

// In-memory store. App-scoped (one Map per server boot) — every
// request reads/writes the same data via the same instance.
@Inject
@Singleton
export class UserRepository {
  private nextId = 3;
  private users: Map<number, User> = new Map([
    [1, { id: 1, name: "Alice", email: "alice@example.com" }],
    [2, { id: 2, name: "Bob", email: "bob@example.com" }],
  ]);

  list(): User[] {
    return Array.from(this.users.values());
  }

  byId(id: number): User | undefined {
    return this.users.get(id);
  }

  create(name: string, email: string): User {
    const user: User = { id: this.nextId++, name, email };
    this.users.set(user.id, user);
    return user;
  }
}
