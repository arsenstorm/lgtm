import type { FormEvent } from "react";
import { useState } from "react";

import { TimeAgo } from "@/components/time-ago";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import type { TodoComment } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

const authorInitials = (author: string | null) =>
  author ? author.slice(0, 2).toUpperCase() : "A";

export function TodoActivity({
  comments,
  createdAt,
  pending,
  onSend,
}: {
  comments: TodoComment[];
  createdAt: number;
  onSend: (body: string) => Promise<boolean>;
  pending: boolean;
}) {
  const [body, setBody] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const text = body.trim();
    if (!text || pending) {
      return;
    }
    if (await onSend(text)) {
      setBody("");
    }
  }

  return (
    <section className="flex min-w-0 flex-col gap-4">
      <h2 className="font-medium text-muted-foreground text-sm">Activity</h2>

      <ol className="flex min-w-0 flex-col gap-4">
        {/* Creation is the only recorded event: the orchestrator keeps no
            history of edits yet. */}
        <li className="flex items-center gap-2 text-sm">
          <span
            aria-hidden="true"
            className="size-1.5 shrink-0 rounded-full bg-border"
          />
          <span className="text-muted-foreground">Created todo</span>
          <TimeAgo at={createdAt} className="text-muted-foreground text-xs" />
        </li>

        {comments.map((comment) => (
          <li className="flex min-w-0 gap-2" key={comment.id}>
            <Avatar size="sm">
              <AvatarFallback>{authorInitials(comment.author)}</AvatarFallback>
            </Avatar>
            <div className="flex min-w-0 flex-col gap-1">
              <div className="flex flex-wrap items-baseline gap-2">
                <span
                  className={cn(
                    "font-medium text-xs",
                    comment.author && "font-mono [overflow-wrap:anywhere]"
                  )}
                >
                  {comment.author ?? "automation"}
                </span>
                <TimeAgo
                  at={comment.created_at}
                  className="text-muted-foreground text-xs"
                />
              </div>
              <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
                {comment.body}
              </p>
            </div>
          </li>
        ))}
      </ol>

      <form className="flex min-w-0 gap-2" onSubmit={submit}>
        <Avatar size="sm">
          <AvatarFallback>AS</AvatarFallback>
        </Avatar>
        <div className="flex min-w-0 flex-1 flex-col items-start gap-2">
          <Textarea
            aria-label="New comment"
            disabled={pending}
            onChange={(event) => setBody(event.target.value)}
            placeholder="Leave a comment…"
            value={body}
          />
          <Button
            disabled={pending || body.trim() === ""}
            size="sm"
            type="submit"
          >
            Comment
          </Button>
        </div>
      </form>
    </section>
  );
}
