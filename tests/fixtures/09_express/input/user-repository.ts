import { Inject, Singleton } from "tsdi";

export interface User {
  id: number;
  name: string;
}

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
