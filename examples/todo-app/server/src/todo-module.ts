import { Module, Provides, Singleton } from "@msulak/anvil";
import { TodoService } from "./todo-service.js";

@Module
export class TodoModule {
  @Singleton
  @Provides
  static todoService(): TodoService {
    const service = new TodoService();
    service.seed();
    return service;
  }
}
