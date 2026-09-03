import { type RefObject, useCallback } from "react";

import type { EditorHeading } from "@/components/markdown-editor";
import { scrollToHeading } from "@/components/markdown-editor";
import { cn } from "@/lib/utils";

const INDENT = ["", "pl-3", "pl-6"];

function TocRow({
  heading,
  containerRef,
}: {
  heading: EditorHeading;
  containerRef: RefObject<HTMLElement | null>;
}) {
  const scroll = useCallback(
    () => scrollToHeading(containerRef.current, heading.index),
    [containerRef, heading.index]
  );

  return (
    <button
      className={cn(
        "truncate text-left text-muted-foreground text-sm transition-colors hover:text-foreground",
        INDENT[heading.level - 1]
      )}
      onClick={scroll}
      type="button"
    >
      {heading.text}
    </button>
  );
}

export function EditorToc({
  headings,
  containerRef,
}: {
  headings: EditorHeading[];
  /** The scrollable/document container holding the editor's rendered headings. */
  containerRef: RefObject<HTMLElement | null>;
}) {
  if (headings.length === 0) {
    return null;
  }

  return (
    <nav aria-label="On this page" className="flex flex-col gap-1">
      <span className="font-medium text-muted-foreground text-xs">
        On this page
      </span>
      {headings.map((heading) => (
        <TocRow
          containerRef={containerRef}
          heading={heading}
          key={heading.index}
        />
      ))}
    </nav>
  );
}

// Scroll-spy (highlighting the section you are reading) is a later nicety; it
// needs an IntersectionObserver over the same headings and nobody has asked yet.
