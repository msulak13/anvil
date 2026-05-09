import { Inject, Singleton } from "@msulak/anvil";

export interface User {
  id: number;
  name: string;
}

// In-memory store. @Singleton ensures every consumer (UserService,
// future request handlers) sees the same Map instance.
@Inject
@Singleton
export class UserRepository {
  private users: Map<number, User> = new Map([
    [1, { id: 1, name: "Alice" }],
    [2, { id: 2, name: "Bob" }],
  ]);

  list(): User[] {
    return Array.from(this.users.values());
  }

  byId(id: number): User | undefined {
    return this.users.get(id);
  }
}
