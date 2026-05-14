import {
  Controller,
  Get,
  Post,
  Put,
  Delete,
  Tag,
  Returns,
} from "@anvil-di/anvil-bellows";
import type { Body, Query, Params, Responds } from "@anvil-di/anvil-bellows";
import type { Response } from "express";
import { z } from "zod";
import type { TodoService } from "./todo-service.js";

// ── Schemas ─────────────────────────────────────────────────────────────────

export const TodoSchema = z.object({
  id: z.string(),
  title: z.string(),
  completed: z.boolean(),
  createdAt: z.string(),
});

export const TodoListSchema = z.object({
  items: z.array(TodoSchema),
});

export const CreateTodoBody = z.object({
  title: z.string().min(1, "Title is required"),
});

export const UpdateTodoBody = z.object({
  title: z.string().min(1).optional(),
  completed: z.boolean().optional(),
});

export const TodoParams = z.object({ id: z.string() });

export const TodoQuery = z.object({
  completed: z
    .enum(["true", "false"])
    .transform((v) => v === "true")
    .optional(),
});

// ── Controller ───────────────────────────────────────────────────────────────

@Controller("/todos")
@Tag("todos")
export class TodoController {
  constructor(private readonly todoService: TodoService) {}

  @Get("/")
  list(query: Query<typeof TodoQuery>): Responds<typeof TodoListSchema> {
    return { items: this.todoService.list(query.completed) };
  }

  @Get("/:id")
  byId(params: Params<typeof TodoParams>): Responds<typeof TodoSchema> {
    return this.todoService.getOrThrow(params.id);
  }

  @Post("/")
  create(body: Body<typeof CreateTodoBody>): Responds<typeof TodoSchema> {
    return this.todoService.create(body.title);
  }

  @Put("/:id")
  update(
    params: Params<typeof TodoParams>,
    body: Body<typeof UpdateTodoBody>,
  ): Responds<typeof TodoSchema> {
    return this.todoService.update(params.id, body);
  }

  @Delete("/:id")
  @Returns(204)
  remove(params: Params<typeof TodoParams>, res: Response): void {
    this.todoService.delete(params.id);
    res.status(204).end();
  }
}
