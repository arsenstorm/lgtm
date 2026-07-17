import { formatDistanceToNow } from "date-fns";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import type { ConversationComment } from "@/types/github";

/**
 * The pull request's issue-level discussion, oldest first, plus a compact
 * composer. Cmd/Ctrl+Enter posts. Bodies are plain text, never HTML.
 */
export function ConversationSection({
  comments,
  busy,
  onAdd,
}: {
  comments: ConversationComment[];
  busy: boolean;
  onAdd: (body: string) => Promise<boolean>;
}) {
  const [body, setBody] = useState("");
  const trimmed = body.trim();

  const submit = async () => {
    if (!trimmed || busy) {
      return;
    }
    const ok = await onAdd(body);
    if (ok) {
      setBody("");
    }
  };

  return (
    <section className="flex flex-col gap-2 border-t p-4">
      <h3 className="font-medium text-sm">Discussion</h3>
      {comments.length === 0 ? (
        <p className="text-muted-foreground text-sm">No discussion yet</p>
      ) : (
        <div className="flex flex-col gap-2">
          {comments.map((comment) => (
            <div
              className="flex flex-col gap-1 rounded-lg border bg-card p-2.5"
              key={comment.id}
            >
              <div className="flex items-center gap-2">
                <span className="font-medium text-sm">
                  {comment.authorLogin}
                </span>
                <span className="ml-auto text-muted-foreground text-xs">
                  {formatDistanceToNow(new Date(comment.createdAt), {
                    addSuffix: true,
                  })}
                </span>
              </div>
              <p className="whitespace-pre-wrap break-words text-sm">
                {comment.body}
              </p>
            </div>
          ))}
        </div>
      )}

      <Textarea
        aria-label="New comment"
        onChange={(event) => setBody(event.target.value)}
        onKeyDown={(event) => {
          if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
            event.preventDefault();
            submit();
          }
        }}
        placeholder="Add a comment…"
        value={body}
      />
      <Button
        className="w-fit"
        disabled={!trimmed || busy}
        onClick={submit}
        size="sm"
        type="button"
      >
        {busy ? <Spinner /> : null}
        Comment
      </Button>
    </section>
  );
}
