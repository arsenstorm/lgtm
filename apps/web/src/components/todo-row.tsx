import { Link } from "@tanstack/react-router";

import { projectName } from "@/components/app-sidebar";
import { PriorityIcon } from "@/components/priority-icon";
import { TimeAgo } from "@/components/time-ago";
import { MARK } from "@/components/todo-chips";
import type { Todo } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

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
    <Link
      className={cn(
        "flex items-center gap-2.5 rounded-md py-1.5 pr-2 pl-7 text-sm hover:bg-foreground/4",
        done && "text-muted-foreground",
        className
      )}
      params={{ id: todo.id }}
      to="/todos/$id"
    >
      <PriorityIcon
        className="size-4 text-muted-foreground"
        label={`${todo.priority} priority`}
        priority={todo.priority}
      />
      <span className="shrink-0 font-mono text-muted-foreground text-xs">
        {todo.display_id}
      </span>
      <Mark
        aria-label={label}
        className={cn("size-4 shrink-0", markClassName)}
        role="img"
        weight={done ? "fill" : "regular"}
      />
      <span className="min-w-0 truncate">{todo.title}</span>
      {todo.blockers.length > 0 && (
        <span className="shrink-0 text-muted-foreground text-xs">
          blocked by {todo.blockers.length}
        </span>
      )}
      <span className="ml-auto hidden shrink-0 text-muted-foreground text-xs sm:block">
        {todo.repository ? projectName(todo.repository) : "every repository"}
      </span>
      <TimeAgo
        at={todo.created_at}
        className="w-16 shrink-0 text-end text-muted-foreground text-xs tabular-nums"
      />
    </Link>
  );
}
