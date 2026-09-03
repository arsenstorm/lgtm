"use client";

import { useEffect, useRef, useState } from "react";
import styles from "./TodoList.module.css";

const LABELS = [
  "Scaffold the project structure",
  "Build the component registry",
  "Implement entitlement gating",
  "Wire up Stripe checkout",
  "Polish the landing page",
];

const START_DELAY = 700;
const STEP_MS = 2250; // how long each task stays "working"

const cls = (base: string, on?: boolean) => base + (on ? " " + styles.on : "");
const CheckIcon = ({ on }: { on?: boolean }) => (
  <svg
    aria-hidden="true"
    className={cls(styles.todoIcon, on)}
    height="16"
    viewBox="0 0 24 24"
    width="16"
  >
    <path
      d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.6"
    />
  </svg>
);
const ArrowIcon = ({ on }: { on?: boolean }) => (
  <svg
    aria-hidden="true"
    className={cls(styles.todoIcon + " " + styles.strong, on)}
    height="16"
    viewBox="0 0 24 24"
    width="16"
  >
    <path
      d="m12.75 15 3-3m0 0-3-3m3 3h-7.5M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.6"
    />
  </svg>
);
const DashedIcon = ({ on }: { on?: boolean }) => (
  <svg
    aria-hidden="true"
    className={cls(styles.todoIcon, on)}
    height="16"
    viewBox="0 0 24 24"
    width="16"
  >
    <circle
      cx="12"
      cy="12"
      fill="none"
      r="9"
      stroke="currentColor"
      strokeDasharray="1.8 3.6"
      strokeLinecap="round"
      strokeWidth="1.8"
    />
  </svg>
);

// one character slot that rolls the old glyph up and the new one in on change
const RollDigit = ({ char }: { char: string }) => {
  const prev = useRef(char);
  const [roll, setRoll] = useState<{ from: string; to: string } | null>(null);
  const [up, setUp] = useState(false);
  useEffect(() => {
    if (char === prev.current) {
      return;
    }
    const from = prev.current;
    prev.current = char;
    setRoll({ from, to: char });
    setUp(false);
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => setUp(true))
    );
    const done = setTimeout(() => setRoll(null), 380);
    return () => {
      cancelAnimationFrame(raf);
      clearTimeout(done);
    };
  }, [char]);
  if (!roll) {
    return <span className={styles.rollDigit}>{char}</span>;
  }
  return (
    <span className={styles.rollDigit}>
      <span className={cls(styles.rollInner, up)}>
        <span>{roll.from}</span>
        <span>{roll.to}</span>
      </span>
    </span>
  );
};
const RollingCount = ({ value }: { value: string }) => (
  <span aria-label={value} className={styles.rollCount}>
    {value.split("").map((c, i) => (
      <RollDigit char={c} key={i} />
    ))}
  </span>
);
const FilledCheckIcon = () => (
  <svg
    aria-hidden="true"
    className={styles.todoHeadCheck}
    height="16"
    viewBox="0 0 24 24"
    width="16"
  >
    <path
      clipRule="evenodd"
      d="M2.25 12c0-5.385 4.365-9.75 9.75-9.75s9.75 4.365 9.75 9.75-4.365 9.75-9.75 9.75S2.25 17.385 2.25 12Zm13.36-1.814a.75.75 0 1 0-1.22-.872l-3.236 4.53L9.53 12.22a.75.75 0 0 0-1.06 1.06l2.25 2.25a.75.75 0 0 0 1.14-.094l3.75-5.25Z"
      fill="currentColor"
      fillRule="evenodd"
    />
  </svg>
);

export interface TodoItem {
  label: string;
  state: "active" | "done" | "pending";
}

export function TodoList({ items }: { items?: TodoItem[] } = {}) {
  const [collapsed, setCollapsed] = useState(false);
  // -1 = not started (plan shown), 0..n-1 = working on that task, n = all done
  const [current, setCurrent] = useState(-1);
  // Without real items the list plays its demo script; with them every state
  // comes from the caller and the timer stays off.
  const scripted = items === undefined;
  const list: TodoItem[] =
    items ?? LABELS.map((label) => ({ label, state: "pending" as const }));
  const n = list.length;

  useEffect(() => {
    if (!scripted) {
      return;
    }
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      setCurrent(n);
      return;
    }
    const timers = [setTimeout(() => setCurrent(0), START_DELAY)];
    for (let i = 0; i < n; i++) {
      timers.push(
        setTimeout(() => setCurrent(i + 1), START_DELAY + (i + 1) * STEP_MS)
      );
    }
    return () => timers.forEach(clearTimeout);
  }, [n, scripted]);

  const doneCount = scripted
    ? Math.min(Math.max(current, 0), n)
    : list.filter((item) => item.state === "done").length;
  const started = scripted
    ? current >= 0
    : list.some((item) => item.state !== "pending");
  const allDone = doneCount >= n;
  const running = started && !allDone;
  const pct = Math.round((doneCount / n) * 100);

  return (
    <div className={styles.todo}>
      <button
        aria-expanded={!collapsed}
        aria-label="Toggle to-dos"
        className={styles.todoHead}
        onClick={() => setCollapsed((c) => !c)}
        type="button"
      >
        <span className={styles.todoHeadIcon}>
          {allDone ? (
            <FilledCheckIcon />
          ) : running ? (
            <span
              aria-hidden="true"
              className={styles.todoHeadPie}
              style={{ ["--todo-pie" as string]: pct + "%" }}
            >
              <svg className={styles.todoHeadPieRing} viewBox="0 0 24 24">
                <circle
                  cx="12"
                  cy="12"
                  fill="none"
                  r="10.5"
                  stroke="currentColor"
                  strokeDasharray="2.2 4.4"
                  strokeLinecap="round"
                  strokeWidth="2.2"
                />
              </svg>
            </span>
          ) : (
            <svg
              aria-hidden="true"
              className={styles.todoListIcon}
              fill="none"
              height="16"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
              viewBox="0 0 24 24"
              width="16"
            >
              <path d="M13 5h8" />
              <path d="M13 12h8" />
              <path d="M13 19h8" />
              <path d="m3 17 2 2 4-4" />
              <path d="m3 7 2 2 4-4" />
            </svg>
          )}
          <svg
            aria-hidden="true"
            className={styles.todoChevron}
            height="16"
            viewBox="0 0 24 24"
            width="16"
          >
            <path
              d="m19.5 8.25-7.5 7.5-7.5-7.5"
              fill="none"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="1.8"
            />
          </svg>
        </span>
        <span className={styles.todoTitle}>To-dos</span>
        <span className={styles.todoCount}>
          <RollingCount value={doneCount + "/" + n} />
        </span>
      </button>

      <div
        className={
          styles.todoCollapsible + (collapsed ? " " + styles.isCollapsed : "")
        }
      >
        <div className={styles.todoInner}>
          <ul className={styles.todoList}>
            {list.map((item, i) => {
              const done = scripted
                ? started && i < current
                : item.state === "done";
              const active = scripted
                ? started && i === current && !allDone
                : item.state === "active";
              return (
                <li
                  className={
                    styles.todoItem +
                    (done
                      ? " " + styles.done
                      : active
                        ? " " + styles.active
                        : "")
                  }
                  key={i}
                  style={{ ["--i" as string]: i }}
                >
                  <span className={styles.todoIconWrap}>
                    <DashedIcon on={!(done || active)} />
                    <ArrowIcon on={active} />
                    <CheckIcon on={done} />
                  </span>
                  <span className={styles.todoLabel} data-label={item.label}>
                    {item.label}
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
      </div>
    </div>
  );
}
