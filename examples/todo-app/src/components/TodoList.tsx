import { useQuery } from "@tanstack/react-query";
import { todoControllerGetListOptions } from "../api/@tanstack/react-query.gen.js";
import { TodoItem } from "./TodoItem.js";

interface Props {
  filter: "all" | "active" | "completed";
}

export function TodoList({ filter }: Props) {
  const query = filter === "all" ? undefined : filter === "completed" ? "true" : "false";

  const { data, isLoading, isError } = useQuery(
    todoControllerGetListOptions(query !== undefined ? { query: { completed: query } } : undefined),
  );

  if (isLoading) return <p style={{ color: "#888" }}>Loading…</p>;
  if (isError) return <p style={{ color: "#c33" }}>Failed to load todos.</p>;

  const items = data?.items ?? [];

  if (items.length === 0) {
    return <p style={{ color: "#aaa", fontStyle: "italic" }}>Nothing here yet.</p>;
  }

  return (
    <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
      {items.map((todo) => (
        <TodoItem key={todo.id} todo={todo} />
      ))}
    </ul>
  );
}
