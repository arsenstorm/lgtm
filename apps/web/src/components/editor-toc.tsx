import {
  type RefObject,
  useCallback,
  useEffect,
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

interface Range {
  first: number;
  last: number;
}

/** The headings on screen: from the section being read, the last heading
 *  whose top has passed the reading line, to the last heading above the
 *  bottom edge. The document scrolls inside whichever ancestor scrolls, so
 *  that is found by walking up rather than assumed to be the window. */
function useVisibleHeadings(
  containerRef: RefObject<HTMLElement | null>,
  headings: EditorHeading[]
): Range {
  const [range, setRange] = useState<Range>({ first: 0, last: 0 });

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
      const edge = scroller?.getBoundingClientRect();
      const line = (edge?.top ?? 0) + READING_LINE;
      const bottom = edge?.bottom ?? window.innerHeight;
      let first = 0;
      let last = 0;
      content.querySelectorAll("h1, h2, h3").forEach((node, i) => {
        const { top } = node.getBoundingClientRect();
        if (top <= line) {
          first = i;
        }
        if (top < bottom) {
          last = i;
        }
      });
      setRange((current) =>
        current.first === first && current.last === last
          ? current
          : { first, last: Math.max(first, last) }
      );
    };
    update();
    const target: HTMLElement | Window = scroller ?? window;
    target.addEventListener("scroll", update, { passive: true });
    return () => target.removeEventListener("scroll", update);
  }, [containerRef, headings]);

  return range;
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
/** The dots that cap the line at either end. */
const DOT = 2;
/** How long the lit stretch takes to slide to its new rows. */
const SLIDE = "duration-150";
/** A dot lights only once the sliding line has reached it, and goes out the
 *  moment the line starts to leave. */
const DOT_LIT =
  "fill-foreground transition-[fill] duration-0 delay-150 motion-reduce:delay-0";
const DOT_UNLIT = "fill-border";

interface Segment {
  bottom: number;
  top: number;
  x: number;
}

/** The line through the first `count` rows. It stops short of the dots at
 *  either end of the whole line, so nothing shows through a translucent dot. */
function linePath(segments: Segment[], count = segments.length): string {
  let d = "";
  let previous: Segment | null = null;
  for (const [i, segment] of segments.slice(0, count).entries()) {
    if (previous === null) {
      d += `M${segment.x} ${segment.top + DOT}`;
    } else if (previous.x !== segment.x) {
      const mid = segment.top + BEND / 2;
      d += `C${previous.x} ${mid} ${segment.x} ${mid} ${segment.x} ${segment.top + BEND}`;
    }
    const end =
      i === segments.length - 1 ? segment.bottom - DOT : segment.bottom;
    d += `L${segment.x} ${end}`;
    previous = segment;
  }
  return d;
}

/** How far along the line each row ends, so the lit stretch can follow the
 *  line through its bends instead of being cut out of it by a box. */
function rowEnds(segments: Segment[]): number[] {
  const probe = document.createElementNS("http://www.w3.org/2000/svg", "path");
  return segments.map((_, i) => {
    probe.setAttribute("d", linePath(segments, i + 1));
    return probe.getTotalLength();
  });
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
  const visible = useVisibleHeadings(containerRef, headings);
  const list = useRef<HTMLDivElement>(null);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [ends, setEnds] = useState<number[]>([]);

  // The rows are measured once laid out, so the line follows whatever height
  // and wrapping they end up with.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the rows change with the headings
  useLayoutEffect(() => {
    const rows = list.current?.querySelectorAll<HTMLElement>("button") ?? [];
    const depth = depths(headings);
    const measured = [...rows].map((row, i) => ({
      bottom: row.offsetTop + row.offsetHeight,
      top: row.offsetTop,
      x: LINE_X[depth[i]],
    }));
    setSegments(measured);
    setEnds(rowEnds(measured));
  }, [headings]);

  if (headings.length === 0) {
    return null;
  }

  const d = linePath(segments);
  const [start] = segments;
  const end = segments.at(-1);
  const total = ends.at(-1) ?? 0;
  const first = Math.min(visible.first, segments.length - 1);
  const last = Math.min(visible.last, segments.length - 1);
  const from = first === 0 ? 0 : (ends[first - 1] ?? 0);
  const to = ends[last] ?? 0;
  const depth = depths(headings);

  return (
    <nav aria-label="Outline" className="flex flex-col gap-2">
      <h2 className="font-medium text-sm tracking-tight">Outline</h2>
      <div className="relative flex flex-col" ref={list}>
        {start && end && ends.length === segments.length ? (
          <svg
            aria-hidden="true"
            className="pointer-events-none absolute top-0 left-0 overflow-visible fill-none stroke-1"
            height={end.bottom}
            width={LINE_WIDTH}
          >
            <path className="stroke-border" d={d} strokeLinecap="round" />
            {/* One dash, as long as the lit rows, placed that far along. */}
            <path
              className={cn(
                "stroke-foreground transition-[stroke-dasharray,stroke-dashoffset] motion-reduce:transition-none",
                SLIDE
              )}
              d={d}
              strokeDasharray={`${to - from} ${total}`}
              strokeDashoffset={-from}
              strokeLinecap="round"
            />
            <circle
              className={first === 0 ? DOT_LIT : DOT_UNLIT}
              cx={start.x}
              cy={start.top}
              r={DOT}
            />
            <circle
              className={last === segments.length - 1 ? DOT_LIT : DOT_UNLIT}
              cx={end.x}
              cy={end.bottom}
              r={DOT}
            />
          </svg>
        ) : null}
        {headings.map((heading, i) => (
          <TocRow
            active={i >= visible.first && i <= visible.last}
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
