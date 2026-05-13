import { useRef } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { todoControllerGetListQueryKey, todoControllerPostCreateMutation } from "../api/@tanstack/react-query.gen.js";

export function AddTodo() {
  const queryClient = useQueryClient();
  const inputRef = useRef<HTMLInputElement>(null);

  const { mutate, isPending } = useMutation({
    ...todoControllerPostCreateMutation(),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: todoControllerGetListQueryKey() });
      if (inputRef.current) inputRef.current.value = "";
    },
  });

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const title = inputRef.current?.value.trim();
    if (!title) return;
    mutate({ body: { title } });
  }

  return (
    <form onSubmit={handleSubmit} style={{ display: "flex", gap: 8, marginBottom: 16 }}>
      <input
        ref={inputRef}
        type="text"
        placeholder="What needs to be done?"
        disabled={isPending}
        style={{ flex: 1, padding: "8px 12px", fontSize: 16, borderRadius: 4, border: "1px solid #ccc" }}
      />
      <button
        type="submit"
        disabled={isPending}
        style={{ padding: "8px 16px", fontSize: 16, borderRadius: 4, cursor: "pointer" }}
      >
        {isPending ? "Adding…" : "Add"}
      </button>
    </form>
  );
}
