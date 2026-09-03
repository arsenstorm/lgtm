"use client";

import { useEffect, useRef, useState } from "react";
import styles from "./ThinkingReasoning.module.css";

const SENTENCES = [
  "Reading the request and the current selection, then locating the jwt.verify call inside the auth middleware.",
  "The verify call sets no algorithms allowlist, so a token signed with 'none' or a weak cipher could be accepted.",
  "Tracing where the signing secret is loaded from and confirming it is never logged or sent back to the client.",
  "Planning to pin the algorithm to HS256 and to validate the issuer and audience claims on every incoming request.",
  "Scanning the existing tests around the middleware so the fix stays covered and nothing downstream regresses.",
  "Drafting the patch with a focused regression test that rejects tampered, expired, and unsigned tokens.",
];

// Per-sentence reveal cadence (ms). Sums to ~5s of "thinking".
const DELAYS = [700, 900, 800, 850, 800, 900];
const THINK_MS = DELAYS.reduce((a, b) => a + b, 0);
const ELAPSED_S = Math.max(1, Math.round(THINK_MS / 1000));
const COLLAPSE_BEAT = 360;

// Geometry - keep in sync with the CSS below.
const SENT_H = 40; // 2 lines × 20px
const GAP = 4;
const MAX_H = 180; // viewport grows with content up to this, then scrolls
const FADE = 16; // top/bottom fade once the viewport is capped

export function ThinkingReasoning({
  sentences,
  seconds,
}: {
  sentences?: string[];
  seconds?: number;
} = {}) {
  const lines = sentences ?? SENTENCES;
  const elapsed = seconds ?? ELAPSED_S;
  // Real reasoning is handed over already finished, so there is nothing to
  // reveal: the block opens straight into its collapsed summary.
  const scripted = sentences === undefined;
  // "thinking" | "done"
  const [phase, setPhase] = useState(scripted ? "thinking" : "done");
  const [revealed, setRevealed] = useState(scripted ? 0 : lines.length);
  // While thinking the reasoning is always open; once done it folds into
  // the summary and the user can toggle it back open.
  const [open, setOpen] = useState(false);
  // Which soft fades to show while scrolling the unfolded reasoning.
  const [fade, setFade] = useState({ bottom: true, top: false });
  const viewportRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!scripted) {
      return;
    }
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      setRevealed(SENTENCES.length);
      setPhase("done");
      return;
    }
    const timers: ReturnType<typeof setTimeout>[] = [];
    const at = (ms: number, fn: () => void) => timers.push(setTimeout(fn, ms));
    let t = 0;
    DELAYS.forEach((d, i) => {
      t += d;
      at(t, () => setRevealed(i + 1));
    });
    at(THINK_MS + COLLAPSE_BEAT, () => setPhase("done"));
    return () => timers.forEach(clearTimeout);
  }, [scripted]);

  const done = phase === "done";
  const expanded = done ? open : true;
  const count = done ? lines.length : revealed;
  const contentH = count > 0 ? count * SENT_H + (count - 1) * GAP : 0;
  const capped = contentH > MAX_H;
  const viewH = capped ? MAX_H : contentH;
  const scrollable = done && open;
  const translate = scrollable ? 0 : capped ? MAX_H - FADE - contentH : 0;

  const showTop = scrollable ? fade.top : capped;
  const showBottom = scrollable ? fade.bottom : capped;
  const mask = capped
    ? `linear-gradient(to bottom, transparent 0, #000 ${showTop ? FADE : 0}px, #000 calc(100% - ${showBottom ? FADE : 0}px), transparent 100%)`
    : "none";

  const onScroll = () => {
    const el = viewportRef.current;
    if (!el) {
      return;
    }
    setFade({
      bottom: el.scrollTop + el.clientHeight < el.scrollHeight - 1,
      top: el.scrollTop > 1,
    });
  };

  const toggle = () => {
    const next = !open;
    if (next) {
      setFade({ bottom: true, top: false });
      if (viewportRef.current) {
        viewportRef.current.scrollTop = 0;
      }
    }
    setOpen(next);
  };

  return (
    <div className={styles.tr}>
      <button
        aria-expanded={expanded}
        aria-label="Toggle thought"
        className={styles.trHeader + (done ? " " + styles.isClickable : "")}
        onClick={done ? toggle : undefined}
        type="button"
      >
        {done ? (
          <span className={styles.trLabel}>
            <span className={styles.trVerb}>Thought</span> for {elapsed}s
          </span>
        ) : (
          <span className={styles.trLabel + " " + styles.trShimmer}>
            Thinking…
          </span>
        )}
        {done && (
          <svg
            aria-hidden="true"
            className={styles.trChevron}
            height="12"
            viewBox="0 0 24 24"
            width="12"
          >
            <path
              d="m4.5 15.75 7.5-7.5 7.5 7.5"
              fill="none"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="1.8"
            />
          </svg>
        )}
      </button>

      <div
        className={
          styles.trCollapsible + (expanded ? "" : " " + styles.isCollapsed)
        }
      >
        <div className={styles.trInner}>
          <div
            className={
              styles.trViewport + (scrollable ? " " + styles.isScroll : "")
            }
            onScroll={scrollable ? onScroll : undefined}
            ref={viewportRef}
            style={{
              height: `${viewH}px`,
              maskImage: mask,
              WebkitMaskImage: mask,
            }}
          >
            <div
              className={styles.trStream}
              style={{ transform: `translateY(${translate}px)` }}
            >
              {lines.slice(0, count).map((line, i) => (
                <p className={styles.trSentence} key={i}>
                  {line}
                </p>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
