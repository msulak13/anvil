import { randomUUID } from "node:crypto";

export interface Todo {
  id: string;
  title: string;
  completed: boolean;
  createdAt: string;
}

export class NotFoundError extends Error {
  readonly status = 404;
  constructor(id: string) {
    super(`Todo "${id}" not found`);
  }
}

export class TodoService {
  private readonly store = new Map<string, Todo>();

  list(completed?: boolean): Todo[] {
    const all = [...this.store.values()];
    if (completed === undefined) return all;
    return all.filter((t) => t.completed === completed);
  }

  getOrThrow(id: string): Todo {
    const todo = this.store.get(id);
    if (!todo) throw new NotFoundError(id);
    return todo;
  }

  create(title: string): Todo {
    const todo: Todo = {
      id: randomUUID(),
      title,
      completed: false,
      createdAt: new Date().toISOString(),
    };
    this.store.set(todo.id, todo);
    return todo;
  }

  update(id: string, patch: { title?: string | undefined; completed?: boolean | undefined }): Todo {
    const todo = this.getOrThrow(id);
    const updated: Todo = {
      ...todo,
      ...(patch.title !== undefined ? { title: patch.title } : {}),
      ...(patch.completed !== undefined ? { completed: patch.completed } : {}),
    };
    this.store.set(id, updated);
    return updated;
  }

  delete(id: string): void {
    if (!this.store.has(id)) throw new NotFoundError(id);
    this.store.delete(id);
  }

  seed(): void {
    this.create("Buy groceries");
    this.create("Walk the dog");
    this.create("Read a book");
  }
}
