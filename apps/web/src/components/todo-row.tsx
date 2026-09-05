import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useCallback, useState } from "react";
import { toast } from "sonner";

import { projectName } from "@/components/app-sidebar";
import { CopyIcon, LinkIcon, TrashIcon } from "@/components/icons";
import { PriorityIcon } from "@/components/priority-icon";
import { TimeAgo } from "@/components/time-ago";
import { MARK, PRIORITY } from "@/components/todo-chips";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { useAction } from "@/hooks/use-action";
import { deleteTodo, updateTodo } from "@/lib/lgtm/server";
import type { Todo, TodoPriority, TodoStatus } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

const STATUS_OPTIONS: TodoStatus[] = ["open", "in_progress", "done"];
const PRIORITY_OPTIONS: TodoPriority[] = ["low", "medium", "high"];

export function TodoRow({
  className,
  todo,
}: {
  className?: string;
  todo: Todo;
}) {
  const { icon: Mark, label, className: markClassName } = MARK[todo.status];
  const done = todo.status === "done";

  return (
    <TodoMenu todo={todo}>
      <ContextMenuTrigger
        render={
          <Link
            className={cn(
              "flex items-center gap-2.5 rounded-md py-1.5 pr-2 pl-7 text-sm hover:bg-foreground/4 data-popup-open:bg-foreground/4",
              done && "text-muted-foreground",
              className
            )}
            params={{ id: todo.id }}
            to="/todos/$id"
          />
        }
      >
        <PriorityIcon
          className="size-4 text-muted-foreground"
          label={`${todo.priority} priority`}
          priority={todo.priority}
        />
        <span className="max-w-16 shrink-0 truncate font-mono text-muted-foreground text-xs">
          {todo.display_id}
        </span>
        <Mark
          aria-label={label}
          className={cn("size-4 shrink-0", markClassName)}
          role="img"
        />
        <span className="min-w-0 truncate">{todo.title}</span>
        {todo.blockers.length > 0 && (
          <span className="max-w-40 shrink-0 truncate text-muted-foreground text-xs">
            blocked by {todo.blockers.length}
          </span>
        )}
        <span className="ml-auto hidden max-w-40 shrink-0 truncate text-muted-foreground text-xs sm:block">
          {todo.repository ? projectName(todo.repository) : "every repository"}
        </span>
        <TimeAgo
          at={todo.created_at}
          className="w-16 shrink-0 truncate text-end text-muted-foreground text-xs tabular-nums"
        />
      </ContextMenuTrigger>
    </TodoMenu>
  );
}

/** Linear's issue menu, cut down to what the orchestrator can do to a todo. */
function TodoMenu({ children, todo }: { children: ReactNode; todo: Todo }) {
  const [armed, setArmed] = useState(false);
  const disarm = useCallback(() => setArmed(false), []);
  const { busy, run } = useAction({ onStart: disarm });

  const patch = useCallback(
    (
      fields: Parameters<typeof updateTodo>[0]["data"]["patch"],
      message: string
    ) =>
      run(
        "patch",
        () => updateTodo({ data: { id: todo.id, patch: fields } }),
        message
      ),
    [run, todo.id]
  );

  const copy = (text: string, message: string) =>
    navigator.clipboard.writeText(text).then(
      () => toast.success(message),
      (error: unknown) =>
        toast.error(error instanceof Error ? error.message : String(error))
    );

  const onStatus = useCallback(
    (status: string) =>
      patch(
        { status: status as TodoStatus },
        `Marked ${MARK[status as TodoStatus].label.toLowerCase()}`
      ),
    [patch]
  );
  const onPriority = useCallback(
    (priority: string) =>
      patch(
        { priority: priority as TodoPriority },
        `Priority set to ${priority}`
      ),
    [patch]
  );
  const onDelete = useCallback(() => {
    if (!armed) {
      setArmed(true);
      return;
    }
    run("delete", () => deleteTodo({ data: todo.id }), "Todo deleted");
  }, [armed, run, todo.id]);

  return (
    <ContextMenu onOpenChange={disarm}>
      {children}
      <ContextMenuContent className="w-48">
        <ContextMenuSub>
          <ContextMenuSubTrigger disabled={busy}>
            <StatusMark status={todo.status} />
            <span>Status</span>
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuRadioGroup onValueChange={onStatus} value={todo.status}>
              {STATUS_OPTIONS.map((status) => (
                <ContextMenuRadioItem closeOnClick key={status} value={status}>
                  <StatusMark status={status} />
                  <span>{MARK[status].label}</span>
                </ContextMenuRadioItem>
              ))}
            </ContextMenuRadioGroup>
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuSub>
          <ContextMenuSubTrigger disabled={busy}>
            <PriorityIcon priority={todo.priority} />
            <span>Priority</span>
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuRadioGroup
              onValueChange={onPriority}
              value={todo.priority}
            >
              {PRIORITY_OPTIONS.map((priority) => (
                <ContextMenuRadioItem
                  closeOnClick
                  key={priority}
                  value={priority}
                >
                  <PriorityIcon priority={priority} />
                  <span>{PRIORITY[priority].label}</span>
                </ContextMenuRadioItem>
              ))}
            </ContextMenuRadioGroup>
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuSeparator />

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <CopyIcon />
            <span>Copy</span>
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuItem onClick={() => copy(todo.display_id, "Copied ID")}>
              <span>Copy ID</span>
            </ContextMenuItem>
            <ContextMenuItem onClick={() => copy(todo.title, "Copied title")}>
              <span>Copy title</span>
            </ContextMenuItem>
            <ContextMenuItem
              onClick={() =>
                copy(
                  `${window.location.origin}/todos/${todo.id}`,
                  "Copied link"
                )
              }
            >
              <LinkIcon />
              <span>Copy link</span>
            </ContextMenuItem>
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuSeparator />

        {/* The first press arms and keeps the menu open; the second deletes.
          Closing the menu any other way disarms. */}
        <ContextMenuItem
          closeOnClick={armed}
          disabled={busy}
          onClick={onDelete}
          variant="destructive"
        >
          <TrashIcon />
          <span>{armed ? "Confirm delete" : "Delete"}</span>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

function StatusMark({ status }: { status: TodoStatus }) {
  const { icon: Icon, className } = MARK[status];
  return <Icon aria-hidden="true" className={className} />;
}
