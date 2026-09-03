import { Plus, Tag, X } from "@phosphor-icons/react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export const TAG_CHIP =
  "inline-flex shrink-0 items-center gap-1 rounded-full border border-border px-2 py-0.5 text-muted-foreground text-xs";

/** Todos and scratchpads edit tags the same way; both detail pages render this
 *  row under their title. */
export function TagsRow({
  tags,
  disabled,
  onChange,
}: {
  disabled: boolean;
  onChange: (next: string[], message: string) => void;
  tags: string[];
}) {
  const [draft, setDraft] = useState<string | null>(null);

  function commit() {
    const tag = (draft ?? "").trim();
    setDraft(null);
    if (tag !== "" && !tags.includes(tag)) {
      onChange([...tags, tag], "Tag added");
    }
  }

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
      {tags.map((tag) => (
        <span className={TAG_CHIP} key={tag}>
          <Tag aria-hidden="true" className="size-3" />
          {tag}
          <button
            aria-label={`Remove ${tag}`}
            className="-mr-1 rounded-full p-0.5 transition-colors hover:text-foreground disabled:opacity-50"
            disabled={disabled}
            onClick={() =>
              onChange(
                tags.filter((other) => other !== tag),
                "Tag removed"
              )
            }
            type="button"
          >
            <X className="size-3" />
          </button>
        </span>
      ))}

      {draft === null ? (
        <Button
          disabled={disabled}
          onClick={() => setDraft("")}
          size="xs"
          variant="ghost"
        >
          <Plus data-icon="inline-start" />
          Add tag
        </Button>
      ) : (
        <Input
          aria-label="New tag"
          autoFocus
          className="h-6 w-32 text-xs md:text-xs"
          // Blur cancels: an abandoned field should not leave a half-typed tag
          // sitting in the row.
          onBlur={() => setDraft(null)}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              commit();
            } else if (event.key === "Escape") {
              setDraft(null);
            }
          }}
          placeholder="tag"
          value={draft}
        />
      )}
    </div>
  );
}
