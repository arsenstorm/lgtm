import { type RefObject, useCallback, useEffect, useState } from "react";

import type { EditorHeading } from "@/components/markdown-editor";
import { scrollToHeading } from "@/components/markdown-editor";
import { cn } from "@/lib/utils";

interface Branch {
  children: Branch[];
  heading: EditorHeading;
}

/** Nests headings by level: each one goes under the nearest shallower
 *  heading above it, so the guide line beside a section's children runs
 *  unbroken from the first to the last. */
function nest(headings: EditorHeading[]): Branch[] {
  const roots: Branch[] = [];
  const open: Branch[] = [];
  for (const heading of headings) {
    const branch: Branch = { children: [], heading };
    while ((open.at(-1)?.heading.level ?? 0) >= heading.level) {
      open.pop();
    }
    (open.at(-1)?.children ?? roots).push(branch);
    open.push(branch);
  }
  return roots;
}

/** How far below the top of the scroll container a heading counts as the
 *  one being read; the same clearance a teleport lands with, plus the
 *  height of the heading itself. */
const READING_LINE = 96;

const SCROLLS = /(auto|scroll)/;

/** The heading being read: the last one whose top has passed the reading
 *  line. The document scrolls inside whichever ancestor scrolls, so that is
 *  found by walking up rather than assumed to be the window. */
function useActiveHeading(
  containerRef: RefObject<HTMLElement | null>,
  headings: EditorHeading[]
): number {
  const [active, setActive] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: new headings mean a new answer, so re-measure when they change
  useEffect(() => {
    const content = containerRef.current;
    if (content === null) {
      return;
    }
    let scroller: HTMLElement | null = content.parentElement;
    while (
      scroller !== null &&
      !SCROLLS.test(getComputedStyle(scroller).overflowY)
    ) {
      scroller = scroller.parentElement;
    }
    const update = () => {
      const line = (scroller?.getBoundingClientRect().top ?? 0) + READING_LINE;
      let index = 0;
      content.querySelectorAll("h1, h2, h3").forEach((node, i) => {
        if (node.getBoundingClientRect().top <= line) {
          index = i;
        }
      });
      setActive(index);
    };
    update();
    const target: HTMLElement | Window = scroller ?? window;
    target.addEventListener("scroll", update, { passive: true });
    return () => target.removeEventListener("scroll", update);
  }, [containerRef, headings]);

  return active;
}

function TocRow({
  heading,
  active,
  containerRef,
}: {
  active: boolean;
  containerRef: RefObject<HTMLElement | null>;
  heading: EditorHeading;
}) {
  const scroll = useCallback(
    () => scrollToHeading(containerRef.current, heading.index),
    [containerRef, heading.index]
  );

  return (
    <button
      aria-current={active ? "location" : undefined}
      className={cn(
        "flex h-7 w-full min-w-0 items-center rounded-md px-2 text-left text-sm transition-colors hover:bg-muted hover:text-foreground",
        active ? "bg-muted text-foreground" : "text-muted-foreground"
      )}
      onClick={scroll}
      type="button"
    >
      <span className="truncate">{heading.text}</span>
    </button>
  );
}

function Branches({
  branches,
  active,
  containerRef,
  nested = false,
}: {
  active: number;
  branches: Branch[];
  containerRef: RefObject<HTMLElement | null>;
  nested?: boolean;
}) {
  return (
    <ul
      className={cn(
        "flex flex-col",
        nested && "ml-3 border-border border-l pl-3"
      )}
    >
      {branches.map((branch) => (
        <li key={branch.heading.index}>
          <TocRow
            active={branch.heading.index === active}
            containerRef={containerRef}
            heading={branch.heading}
          />
          {branch.children.length > 0 && (
            <Branches
              active={active}
              branches={branch.children}
              containerRef={containerRef}
              nested
            />
          )}
        </li>
      ))}
    </ul>
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
  const active = useActiveHeading(containerRef, headings);
  if (headings.length === 0) {
    return null;
  }

  return (
    <nav aria-label="Outline" className="flex flex-col gap-2">
      <h2 className="font-medium text-sm tracking-tight">Outline</h2>
      <Branches
        active={active}
        branches={nest(headings)}
        containerRef={containerRef}
      />
    </nav>
  );
}
