import {
  type RefObject,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import type { EditorHeading } from "@/components/markdown-editor";
import { scrollToHeading } from "@/components/markdown-editor";
import { cn } from "@/lib/utils";

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

/** Where the line runs for each depth, and how far the text sits past it.
 *  One line threads every row, bending as the depth changes. */
const LINE_X = [1, 13, 25];

/** A document that opens with an H2 is not indented for the H1 it lacks:
 *  depth counts from the shallowest heading present. */
function depths(headings: EditorHeading[]): number[] {
  const shallowest = Math.min(...headings.map((heading) => heading.level));
  return headings.map((heading) =>
    Math.min(heading.level - shallowest, LINE_X.length - 1)
  );
}
const TEXT_GAP = 12;
const LINE_WIDTH = (LINE_X.at(-1) ?? 0) + 3;
/** How much of a row the bend into a new level takes. */
const BEND = 12;

interface Segment {
  bottom: number;
  top: number;
  x: number;
}

function linePath(segments: Segment[]): string {
  let d = "";
  let previous: Segment | null = null;
  for (const segment of segments) {
    if (previous === null) {
      d += `M${segment.x} ${segment.top}`;
    } else if (previous.x !== segment.x) {
      const mid = segment.top + BEND / 2;
      d += `C${previous.x} ${mid} ${segment.x} ${mid} ${segment.x} ${segment.top + BEND}`;
    }
    d += `L${segment.x} ${segment.bottom}`;
    previous = segment;
  }
  return d;
}

function TocRow({
  heading,
  depth,
  active,
  containerRef,
}: {
  active: boolean;
  containerRef: RefObject<HTMLElement | null>;
  depth: number;
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
        "flex h-7 w-full min-w-0 items-center text-left text-sm transition-colors hover:text-foreground",
        active ? "text-foreground" : "text-muted-foreground"
      )}
      onClick={scroll}
      style={{ paddingLeft: LINE_X[depth] + TEXT_GAP }}
      type="button"
    >
      <span className="truncate">{heading.text}</span>
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
  const active = useActiveHeading(containerRef, headings);
  const list = useRef<HTMLDivElement>(null);
  const [segments, setSegments] = useState<Segment[]>([]);
  const clipId = useId();

  // The rows are measured once laid out, so the line follows whatever height
  // and wrapping they end up with.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the rows change with the headings
  useLayoutEffect(() => {
    const rows = list.current?.querySelectorAll<HTMLElement>("button") ?? [];
    const depth = depths(headings);
    setSegments(
      [...rows].map((row, i) => ({
        bottom: row.offsetTop + row.offsetHeight,
        top: row.offsetTop,
        x: LINE_X[depth[i]],
      }))
    );
  }, [headings]);

  if (headings.length === 0) {
    return null;
  }

  const d = linePath(segments);
  const height = segments.at(-1)?.bottom ?? 0;
  const current = segments[active];
  const depth = depths(headings);

  return (
    <nav aria-label="Outline" className="flex flex-col gap-2">
      <h2 className="font-medium text-sm tracking-tight">Outline</h2>
      <div className="relative flex flex-col" ref={list}>
        {segments.length > 0 ? (
          <svg
            aria-hidden="true"
            className="pointer-events-none absolute top-0 left-0 fill-none stroke-1"
            height={height}
            width={LINE_WIDTH}
          >
            <path className="stroke-border" d={d} strokeLinecap="round" />
            {current ? (
              <>
                <clipPath id={clipId}>
                  <rect
                    className="transition-[y,height] duration-150 motion-reduce:transition-none"
                    height={current.bottom - current.top}
                    width={LINE_WIDTH}
                    x={0}
                    y={current.top}
                  />
                </clipPath>
                <path
                  className="stroke-foreground"
                  clipPath={`url(#${clipId})`}
                  d={d}
                  strokeLinecap="round"
                />
              </>
            ) : null}
          </svg>
        ) : null}
        {headings.map((heading, i) => (
          <TocRow
            active={heading.index === active}
            containerRef={containerRef}
            depth={depth[i]}
            heading={heading}
            key={heading.index}
          />
        ))}
      </div>
    </nav>
  );
}
