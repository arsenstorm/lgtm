import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

type CommentComposerProps = {
  caption: string;
  initialBody?: string;
  submitLabel?: string;
  onSubmit: (body: string) => void;
  onCancel: () => void;
};

/**
 * Compact inline composer used both for new comments and for editing in place.
 * Cmd/Ctrl+Enter submits, Escape cancels; both are handled locally so they
 * never reach the global review shortcuts. Pointer events are stopped at the
 * root so interacting with the composer does not clear the diff selection.
 */
export function CommentComposer({
  caption,
  initialBody = "",
  submitLabel = "Comment",
  onSubmit,
  onCancel,
}: CommentComposerProps) {
  const [body, setBody] = useState(initialBody);
  const ref = useRef<HTMLTextAreaElement>(null);
  const trimmed = body.trim();

  useEffect(() => {
    const node = ref.current;
    if (node) {
      node.focus();
      node.setSelectionRange(node.value.length, node.value.length);
    }
  }, []);

  const submit = () => {
    if (trimmed) {
      onSubmit(body);
    }
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    <div
      className="flex flex-col gap-2 rounded-lg border bg-card p-2.5 text-card-foreground shadow-sm"
      onMouseDown={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <p className="font-medium text-muted-foreground text-xs">{caption}</p>
      <Textarea
        className="min-h-16 bg-background text-sm"
        onChange={(event) => setBody(event.target.value)}
        onKeyDown={(event) => {
          if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
            event.preventDefault();
            submit();
          } else if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            onCancel();
          }
        }}
        placeholder="Leave a review comment…"
        ref={ref}
        value={body}
      />
      <div className="flex items-center gap-2">
        <span className="mr-auto text-muted-foreground text-xs">
          ⌘↵ to {submitLabel.toLowerCase()}
        </span>
        <Button onClick={onCancel} size="xs" type="button" variant="ghost">
          Cancel
        </Button>
        <Button disabled={!trimmed} onClick={submit} size="xs" type="button">
          {submitLabel}
        </Button>
      </div>
    </div>
  );
}
