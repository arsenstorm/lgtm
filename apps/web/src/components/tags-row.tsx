import { useLayoutEffect, useRef, useState } from "react";
import { PlusIcon, TagIcon, XIcon } from "@/components/icons";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export const TAG_CHIP =
  "inline-flex h-5 min-w-0 max-w-full shrink-0 items-center gap-1 overflow-hidden rounded-full border border-border px-2 text-muted-foreground text-xs";

/** A tag that no longer fits its chip says its whole name on hover. */
function TagName({ tag }: { tag: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [clipped, setClipped] = useState(false);

  useLayoutEffect(() => {
    const el = ref.current;
    if (el === null) {
      return;
    }
    const check = () => setClipped(el.scrollWidth > el.clientWidth);
    check();
    const observer = new ResizeObserver(check);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return (
    <Tooltip disabled={!clipped}>
      <TooltipTrigger render={<span className="truncate" ref={ref} />}>
        {tag}
      </TooltipTrigger>
      <TooltipContent>{tag}</TooltipContent>
    </Tooltip>
  );
}

/** Past this many rows the rest of the tags fold behind a count. */
const MAX_ROWS = 2;

/** Todos and scratchpads edit tags the same way. Every piece is one chip
 *  tall, so swapping the plus for the field moves nothing around it. */
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
  const [expanded, setExpanded] = useState(false);
  // How many chips fit in MAX_ROWS with the controls after them. Starts at
  // all of them and is cut back one at a time until it fits. The cutting
  // renders the folded row even while expanded, but only in layout effects,
  // so nothing of it reaches the screen.
  const [shown, setShown] = useState(tags.length);
  const [fitting, setFitting] = useState(true);
  const [measured, setMeasured] = useState(tags);
  const row = useRef<HTMLDivElement>(null);
  if (measured !== tags) {
    setMeasured(tags);
    setShown(tags.length);
    setFitting(true);
  }

  useLayoutEffect(() => {
    const el = row.current;
    if (!fitting || el === null) {
      return;
    }
    const rows = new Set(
      [...el.children].map((child) => (child as HTMLElement).offsetTop)
    );
    if (rows.size > MAX_ROWS && shown > 0) {
      setShown(shown - 1);
    } else {
      setFitting(false);
    }
  }, [fitting, shown]);

  useLayoutEffect(() => {
    const el = row.current;
    if (el === null) {
      return;
    }
    // Only a new width can change what fits; the height changes whenever
    // chips fold, and reacting to that would fold and unfold forever.
    let width = el.clientWidth;
    const observer = new ResizeObserver(() => {
      if (el.clientWidth !== width) {
        width = el.clientWidth;
        setShown(tags.length);
        setFitting(true);
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [tags.length]);

  const folded = tags.length - shown;

  function commit() {
    const tag = (draft ?? "").trim();
    setDraft(null);
    if (tag !== "" && !tags.includes(tag)) {
      onChange([...tags, tag], "Tag added");
    }
  }

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5" ref={row}>
      {(expanded && !fitting ? tags : tags.slice(0, shown)).map((tag) => (
        <span className={TAG_CHIP} key={tag}>
          <TagIcon aria-hidden="true" className="size-3" />
          <TagName tag={tag} />
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
            <XIcon className="size-3" />
          </button>
        </span>
      ))}

      {folded > 0 && (
        <button
          className="h-5 rounded-full px-1 text-muted-foreground text-xs transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          onClick={() => setExpanded(!expanded)}
          type="button"
        >
          {expanded ? "Show less" : `${folded} more`}
        </button>
      )}

      {draft === null ? (
        // The hit area grows past the 20px glyph only as far as the gap to
        // the chip beside it allows.
        <button
          aria-label="Add tag"
          className="relative flex size-5 items-center justify-center rounded-full text-muted-foreground transition-colors before:absolute before:-inset-1 hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-50"
          disabled={disabled}
          onClick={() => setDraft("")}
          type="button"
        >
          <PlusIcon className="size-3.5" />
        </button>
      ) : (
        <input
          aria-label="New tag"
          autoFocus
          className="h-5 w-24 bg-transparent text-base outline-none placeholder:text-muted-foreground md:text-xs"
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
          spellCheck={false}
          value={draft}
        />
      )}
    </div>
  );
}
