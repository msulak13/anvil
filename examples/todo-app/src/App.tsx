import { useState } from "react";
import { AddTodo } from "./components/AddTodo.js";
import { TodoList } from "./components/TodoList.js";

type Filter = "all" | "active" | "completed";

const FILTERS: Filter[] = ["all", "active", "completed"];

export default function App() {
  const [filter, setFilter] = useState<Filter>("all");

  return (
    <div
      style={{
        maxWidth: 540,
        margin: "60px auto",
        padding: "0 16px",
        fontFamily: "system-ui, sans-serif",
      }}
    >
      <h1 style={{ fontSize: 32, marginBottom: 24, fontWeight: 700 }}>todos</h1>

      <AddTodo />

      <div style={{ display: "flex", gap: 8, marginBottom: 16 }}>
        {FILTERS.map((f) => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            style={{
              padding: "4px 12px",
              borderRadius: 4,
              border: "1px solid",
              borderColor: filter === f ? "#555" : "#ccc",
              background: filter === f ? "#555" : "transparent",
              color: filter === f ? "#fff" : "inherit",
              cursor: "pointer",
              textTransform: "capitalize",
            }}
          >
            {f}
          </button>
        ))}
      </div>

      <TodoList filter={filter} />
    </div>
  );
}
