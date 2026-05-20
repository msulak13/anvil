import { Inject } from "@anvil-di/anvil";
import { Logger } from "./logger";
import { User, UserRepository } from "./user-repository";

// Multiple constructor deps: the parser lowers each parameter type to
// a Key, and the dagger threads `getUserRepository()` and `getLogger()`
// into the `new UserService(...)` call.
@Inject
export class UserService {
  constructor(
    private repo: UserRepository,
    private log: Logger,
  ) {}

  list(): User[] {
    this.log.info("UserService.list");
    return this.repo.list();
  }

  byId(id: number): User | undefined {
    this.log.info(`UserService.byId(${id})`);
    return this.repo.byId(id);
  }
}
