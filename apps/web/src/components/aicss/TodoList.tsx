"use client";

import { useEffect, useRef, useState } from "react";
import {
  ChevronIcon,
  CircleArrowRightIcon,
  CircleCheckFilledIcon,
  CircleCheckIcon,
  CircleDashedIcon,
  ListCheckboxIcon,
} from "@/components/icons";
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
  <CircleCheckIcon
    className={cls(styles.todoIcon, on)}
    height={16}
    width={16}
  />
);
const ArrowIcon = ({ on }: { on?: boolean }) => (
  <CircleArrowRightIcon
    className={cls(styles.todoIcon + " " + styles.strong, on)}
    height={16}
    width={16}
  />
);
const DashedIcon = ({ on }: { on?: boolean }) => (
  <CircleDashedIcon
    className={cls(styles.todoIcon, on)}
    height={16}
    width={16}
  />
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
  <CircleCheckFilledIcon
    className={styles.todoHeadCheck}
    height={16}
    width={16}
  />
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
            <ListCheckboxIcon
              className={styles.todoListIcon}
              height={16}
              width={16}
            />
          )}
          <ChevronIcon
            className={styles.todoChevron}
            direction="down"
            height={16}
            width={16}
          />
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
