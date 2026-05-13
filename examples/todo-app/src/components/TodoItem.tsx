import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  todoControllerGetListQueryKey,
  todoControllerDeleteRemoveMutation,
  todoControllerPutUpdateMutation,
} from "../api/@tanstack/react-query.gen.js";
import type { TodoSchema } from "../api/types.gen.js";

interface Props {
  todo: TodoSchema;
}

export function TodoItem({ todo }: Props) {
  const queryClient = useQueryClient();

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: todoControllerGetListQueryKey() });

  const { mutate: toggle, isPending: isToggling } = useMutation({
    ...todoControllerPutUpdateMutation(),
    onSuccess: invalidate,
  });

  const { mutate: remove, isPending: isRemoving } = useMutation({
    ...todoControllerDeleteRemoveMutation(),
    onSuccess: invalidate,
  });

  const isPending = isToggling || isRemoving;

  return (
    <li
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 0",
        borderBottom: "1px solid #eee",
        opacity: isPending ? 0.5 : 1,
      }}
    >
      <input
        type="checkbox"
        checked={todo.completed}
        disabled={isPending}
        onChange={() =>
          toggle({ path: { id: todo.id }, body: { completed: !todo.completed } })
        }
        style={{ width: 18, height: 18, cursor: "pointer" }}
      />
      <span
        style={{
          flex: 1,
          fontSize: 16,
          textDecoration: todo.completed ? "line-through" : "none",
          color: todo.completed ? "#aaa" : "inherit",
        }}
      >
        {todo.title}
      </span>
      <button
        onClick={() => remove({ path: { id: todo.id } })}
        disabled={isPending}
        aria-label="Delete"
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          fontSize: 18,
          color: "#e55",
          lineHeight: 1,
        }}
      >
        ✕
      </button>
    </li>
  );
}
