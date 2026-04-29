import { Inject } from "tsdi";
import { Logger } from "./logger";
import { User, UserRepository } from "./user-repository";

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
