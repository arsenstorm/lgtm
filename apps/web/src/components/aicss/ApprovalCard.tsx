"use client";

import {
  ArrowElbowDownLeft,
  ArrowsOutSimple,
  CaretDown,
  CaretUp,
  ChatsCircle,
  CheckSquare,
  ListChecks,
  Terminal,
  X,
} from "@phosphor-icons/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import styles from "./ApprovalCard.module.css";

export type ApprovalVariant = "questions" | "command" | "plan";

export interface ApprovalQuestion {
  id: string;
  options: string[];
  prompt: string;
}

export interface ApprovalPlanStep {
  detail?: string;
  id: string;
  title: string;
}

const DEFAULT_QUESTIONS: ApprovalQuestion[] = [
  {
    id: "q1",
    options: ["Session cookies", "JWT bearer", "OAuth only"],
    prompt: "Which auth approach should we use?",
  },
  {
    id: "q2",
    options: [".env.local", "Vault / secrets manager", "CI only"],
    prompt: "Where should secrets live?",
  },
  {
    id: "q3",
    options: ["Yes - gradual rollout", "No - full release"],
    prompt: "Ship behind a feature flag?",
  },
];

const DEFAULT_COMMAND = "pnpm db:migrate && pnpm build";

const DEFAULT_PLAN: ApprovalPlanStep[] = [
  {
    detail: "Create + apply SQL, keep rollback script",
    id: "p1",
    title: "Add migration for sessions table",
  },
  {
    detail: "Protect /account and /api/checkout",
    id: "p2",
    title: "Wire auth middleware",
  },
  {
    detail: "Magic-link path and happy-path e2e",
    id: "p3",
    title: "Update login flow + tests",
  },
  {
    detail: "Profile, sessions, and danger zone",
    id: "p4",
    title: "Add account settings page",
  },
  {
    detail: "Protect auth and checkout endpoints",
    id: "p5",
    title: "Tighten CSRF + rate limits",
  },
  {
    detail: "Changelog + support snippet",
    id: "p6",
    title: "Write rollout notes",
  },
];

const DEFAULT_PLAN_PREVIEW = 3;
const DEFAULT_PLAN_TITLE = "Session auth migration";
const DEFAULT_PLAN_SUMMARY =
  "Ship cookie-based sessions with middleware and tests.\nIncludes a safe rollout path for production.";
const AUTO_APPROVE_SECS = 30;
const ADVANCE_MS = 320;
const ROLL_MS = 400;

function RollingDigits({ value }: { value: string }) {
  const prevRef = useRef(value);
  const [oldVal, setOldVal] = useState(value);
  const [newVal, setNewVal] = useState(value);
  const [rolling, setRolling] = useState(false);
  const [shifted, setShifted] = useState(false);
  const [dir, setDir] = useState<"up" | "down">("up");

  useEffect(() => {
    if (prevRef.current === value) {
      return;
    }
    const from = prevRef.current;
    prevRef.current = value;
    const fromN = Number.parseInt(from, 10);
    const toN = Number.parseInt(value, 10);
    setDir(
      Number.isFinite(fromN) && Number.isFinite(toN) && toN < fromN
        ? "down"
        : "up"
    );
    setOldVal(from);
    setNewVal(value);
    setRolling(true);
    setShifted(false);

    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => setShifted(true));
    });
    const done = setTimeout(() => {
      setRolling(false);
      setOldVal(value);
      setShifted(false);
    }, ROLL_MS);

    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
      clearTimeout(done);
    };
  }, [value]);

  const chars = rolling ? newVal : oldVal;

  return (
    <>
      {Array.from({ length: chars.length }, (_, i) => {
        const o = oldVal[i] ?? "";
        const n = chars[i] ?? "";
        if (!rolling || o === n) {
          return (
            <span className={styles.digitStatic} key={`${i}-${n}`}>
              {n}
            </span>
          );
        }
        const top = dir === "down" ? n : o;
        const bottom = dir === "down" ? o : n;
        return (
          <span className={styles.digitRoll} key={`${i}-${o}-${n}-${dir}`}>
            <span
              className={styles.digitRollInner}
              data-dir={dir}
              data-shifted={shifted ? "true" : undefined}
            >
              <span>{top}</span>
              <span>{bottom}</span>
            </span>
          </span>
        );
      })}
    </>
  );
}

function TodoDashedIcon() {
  const dots = 12;
  const dash = 0.022;
  const gap = 1 / dots - dash;
  return (
    <svg
      aria-hidden="true"
      className={styles.todoIcon}
      height="16"
      viewBox="0 0 24 24"
      width="16"
    >
      <circle
        cx="12"
        cy="12"
        fill="none"
        pathLength={1}
        r="9"
        stroke="currentColor"
        strokeDasharray={`${dash} ${gap}`}
        strokeLinecap="round"
        strokeWidth="1.8"
      />
    </svg>
  );
}

export interface ApprovalCardProps {
  approveLabel?: string;
  /** Seconds before the plan approves itself; `null` never auto-approves. */
  autoApproveSeconds?: number | null;
  className?: string;
  command?: string;
  cwd?: string;
  onApprove?: (payload?: { answers?: Record<string, string> }) => void;
  onReject?: () => void;
  plan?: ApprovalPlanStep[];
  planPreviewCount?: number;
  planSummary?: string;
  planTitle?: string;
  questions?: ApprovalQuestion[];
  rejectLabel?: string;
  title?: string;
  variant?: ApprovalVariant;
}

export function ApprovalCard({
  variant = "questions",
  questions = DEFAULT_QUESTIONS,
  command = DEFAULT_COMMAND,
  cwd = "~/aicss",
  plan = DEFAULT_PLAN,
  planTitle,
  planSummary,
  planPreviewCount = DEFAULT_PLAN_PREVIEW,
  title,
  approveLabel,
  rejectLabel,
  autoApproveSeconds = AUTO_APPROVE_SECS,
  onApprove,
  onReject,
  className,
}: ApprovalCardProps) {
  const autoTotal = autoApproveSeconds ?? 0;
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [otherSelected, setOtherSelected] = useState<Record<string, boolean>>(
    {}
  );
  const [customDraft, setCustomDraft] = useState<Record<string, string>>({});
  const [step, setStep] = useState(0);
  const [planExpanded, setPlanExpanded] = useState(false);
  const [autoSecs, setAutoSecs] = useState(autoTotal);
  const [autoUI, setAutoUI] = useState<"active" | "leaving" | "gone">(
    autoTotal > 0 ? "active" : "gone"
  );
  const advanceTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const autoFadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const autoFired = useRef(false);
  const questionRefs = useRef<(HTMLDivElement | null)[]>([]);
  const customInputRefs = useRef<Record<string, HTMLInputElement | null>>({});
  const qMeasured = useRef(false);
  const [qViewportH, setQViewportH] = useState<number | undefined>(undefined);
  const [qTrackY, setQTrackY] = useState(0);
  const [qAnimate, setQAnimate] = useState(false);

  useEffect(
    () => () => {
      if (advanceTimer.current) {
        clearTimeout(advanceTimer.current);
      }
      if (autoFadeTimer.current) {
        clearTimeout(autoFadeTimer.current);
      }
    },
    []
  );

  const safeStep = Math.min(step, Math.max(questions.length - 1, 0));
  const allAnswered =
    questions.length > 0 &&
    questions.every((q) => Boolean(answers[q.id]?.trim()));
  const stepLabel = `${safeStep + 1} / ${questions.length}`;

  const isOtherChoice = (q: ApprovalQuestion) => {
    if (otherSelected[q.id]) {
      return true;
    }
    const a = answers[q.id];
    return Boolean(a) && !q.options.includes(a);
  };

  const syncQuestionSlide = (animate: boolean) => {
    const item = questionRefs.current[safeStep];
    if (!item) {
      return;
    }
    const reduce =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    setQViewportH(item.offsetHeight + 2);
    setQTrackY(item.offsetTop);
    setQAnimate(animate && !reduce);
  };

  useLayoutEffect(() => {
    if (variant !== "questions") {
      qMeasured.current = false;
      setQViewportH(undefined);
      setQTrackY(0);
      setQAnimate(false);
      return;
    }
    const animate = qMeasured.current;
    qMeasured.current = true;
    syncQuestionSlide(animate);
  }, [variant, safeStep, questions, answers]);

  useEffect(() => {
    if (variant !== "questions") {
      return;
    }
    const id = requestAnimationFrame(() =>
      syncQuestionSlide(qMeasured.current)
    );
    return () => cancelAnimationFrame(id);
  }, [variant, safeStep, questions]);

  const previewCount = Math.max(0, planPreviewCount);
  const planPreview = plan.slice(0, previewCount);
  const planRest = plan.slice(previewCount);
  const hasPlanMore = planRest.length > 0;
  const showPlanRest = planExpanded || !hasPlanMore;

  const resolvedPlanTitle = planTitle ?? DEFAULT_PLAN_TITLE;
  const resolvedPlanSummary = planSummary ?? DEFAULT_PLAN_SUMMARY;

  const resolvedTitle =
    title ??
    (variant === "questions"
      ? "Questions"
      : variant === "command"
        ? "Run this command?"
        : "Plan Overview");

  const resolvedApprove =
    approveLabel ??
    (variant === "questions"
      ? "Continue"
      : variant === "command"
        ? "Run"
        : "Approve");

  const resolvedReject =
    rejectLabel ?? (variant === "plan" ? "View Plan" : "Skip");

  const canContinue = variant !== "questions" || allAnswered;

  const handleApprove = (nextAnswers?: Record<string, string>) => {
    if (variant === "questions") {
      const a = nextAnswers ?? answers;
      const ok = questions.every((q) => Boolean(a[q.id]?.trim()));
      if (!ok) {
        return;
      }
      onApprove?.({ answers: a });
      return;
    }
    onApprove?.();
  };

  const handleReject = () => {
    onReject?.();
  };

  const cancelAutoApprove = () => {
    if (autoUI !== "active") {
      return;
    }
    autoFired.current = true;
    setAutoUI("leaving");
    if (autoFadeTimer.current) {
      clearTimeout(autoFadeTimer.current);
    }
    autoFadeTimer.current = setTimeout(() => setAutoUI("gone"), 280);
  };

  useEffect(() => {
    if (variant !== "plan" || autoUI !== "active") {
      return;
    }
    const id = window.setInterval(() => {
      setAutoSecs((s) => Math.max(0, s - 1));
    }, 1000);
    return () => window.clearInterval(id);
  }, [variant, autoUI]);

  useEffect(() => {
    if (variant !== "plan" || autoUI !== "active") {
      return;
    }
    if (autoSecs > 0 || autoFired.current) {
      return;
    }
    autoFired.current = true;
    onApprove?.();
  }, [autoSecs, variant, autoUI, onApprove]);

  const selectOption = (questionId: string, opt: string) => {
    setOtherSelected((prev) => ({ ...prev, [questionId]: false }));
    setAnswers((prev) => ({ ...prev, [questionId]: opt }));
    if (safeStep < questions.length - 1) {
      if (advanceTimer.current) {
        clearTimeout(advanceTimer.current);
      }
      advanceTimer.current = setTimeout(() => {
        setStep((s) => Math.min(s + 1, questions.length - 1));
      }, ADVANCE_MS);
    }
  };

  const selectOther = (questionId: string) => {
    if (advanceTimer.current) {
      clearTimeout(advanceTimer.current);
    }
    setOtherSelected((prev) => ({ ...prev, [questionId]: true }));
    const draft = customDraft[questionId]?.trim() ?? "";
    setAnswers((prev) => {
      const next = { ...prev };
      if (draft) {
        next[questionId] = draft;
      } else {
        delete next[questionId];
      }
      return next;
    });
    requestAnimationFrame(() => {
      customInputRefs.current[questionId]?.focus();
    });
  };

  const updateCustom = (questionId: string, text: string) => {
    setCustomDraft((prev) => ({ ...prev, [questionId]: text }));
    setOtherSelected((prev) => ({ ...prev, [questionId]: true }));
    setAnswers((prev) => {
      const next = { ...prev };
      const trimmed = text.trim();
      if (trimmed) {
        next[questionId] = trimmed;
      } else {
        delete next[questionId];
      }
      return next;
    });
  };

  const commitCustom = (questionId: string, raw?: string) => {
    const text = (
      raw ??
      customDraft[questionId] ??
      answers[questionId] ??
      ""
    ).trim();
    if (!text) {
      return;
    }
    setCustomDraft((prev) => ({
      ...prev,
      [questionId]: raw ?? prev[questionId] ?? text,
    }));
    setOtherSelected((prev) => ({ ...prev, [questionId]: true }));
    const nextAnswers = { ...answers, [questionId]: text };
    setAnswers(nextAnswers);
    if (safeStep < questions.length - 1) {
      if (advanceTimer.current) {
        clearTimeout(advanceTimer.current);
      }
      setStep((s) => Math.min(s + 1, questions.length - 1));
      return;
    }
    handleApprove(nextAnswers);
  };

  const goToStep = (next: number) => {
    if (advanceTimer.current) {
      clearTimeout(advanceTimer.current);
    }
    setStep(Math.min(Math.max(next, 0), questions.length - 1));
  };

  const Icon =
    variant === "questions"
      ? ChatsCircle
      : variant === "command"
        ? Terminal
        : CheckSquare;

  return (
    <div
      className={`${styles.card}${className ? ` ${className}` : ""}`}
      data-variant={variant}
      onKeyDown={(e) => {
        if (e.key !== "Enter") {
          return;
        }
        if (variant !== "questions") {
          return;
        }
        if (safeStep !== questions.length - 1 || !canContinue) {
          return;
        }
        const el = e.target as HTMLElement;
        if (el.tagName === "INPUT" || el.tagName === "TEXTAREA") {
          return;
        }
        if (
          el.closest(`.${styles.btnGhost}`) ||
          el.closest(`.${styles.btnPrimary}`)
        ) {
          return;
        }
        e.preventDefault();
        handleApprove();
      }}
    >
      <div className={styles.head}>
        <span className={styles.icon} data-variant={variant}>
          <Icon aria-hidden className={styles.iconSvg} />
        </span>
        <div className={styles.headText}>
          <div className={styles.title}>{resolvedTitle}</div>
        </div>
        {variant === "plan" && (
          <div className={styles.headActions}>
            <button
              aria-label="Expand plan"
              className={styles.headAction}
              onClick={(e) => {
                e.preventDefault();
                setPlanExpanded(true);
              }}
              type="button"
            >
              <ArrowsOutSimple
                aria-hidden
                className={styles.headActionIcon}
                strokeWidth={2}
              />
            </button>
          </div>
        )}
      </div>

      {variant === "questions" && questions.length > 0 && (
        <div
          aria-live="polite"
          className={styles.questionsViewport}
          data-animate={qAnimate ? "true" : undefined}
          style={qViewportH == null ? undefined : { height: qViewportH }}
        >
          <div
            className={styles.questionsTrack}
            data-animate={qAnimate ? "true" : undefined}
            style={{ transform: `translate3d(0, ${-qTrackY}px, 0)` }}
          >
            {questions.map((q, qi) => {
              const active = qi === safeStep;
              return (
                <div
                  aria-hidden={active ? undefined : true}
                  className={styles.question}
                  data-active={active ? "true" : undefined}
                  key={q.id}
                  ref={(el) => {
                    questionRefs.current[qi] = el;
                  }}
                >
                  <div className={styles.qPrompt}>{q.prompt}</div>
                  <div
                    aria-label={q.prompt}
                    className={styles.options}
                    role="radiogroup"
                  >
                    {q.options.map((opt, oi) => {
                      const selected =
                        answers[q.id] === opt && !isOtherChoice(q);
                      const letter = String.fromCharCode(65 + oi);
                      return (
                        <button
                          aria-checked={selected}
                          className={styles.option}
                          data-selected={selected ? "true" : undefined}
                          key={opt}
                          onClick={(e) => {
                            e.preventDefault();
                            if (!active) {
                              return;
                            }
                            selectOption(q.id, opt);
                          }}
                          role="radio"
                          tabIndex={active ? 0 : -1}
                          type="button"
                        >
                          <span aria-hidden className={styles.key}>
                            {letter}
                          </span>
                          {opt}
                        </button>
                      );
                    })}
                    {(() => {
                      const otherLetter = String.fromCharCode(
                        65 + q.options.length
                      );
                      const otherOn = isOtherChoice(q);
                      const draft =
                        customDraft[q.id] ??
                        (otherOn &&
                        answers[q.id] &&
                        !q.options.includes(answers[q.id])
                          ? answers[q.id]
                          : "");
                      return (
                        <div
                          aria-checked={otherOn}
                          className={styles.option}
                          data-other="true"
                          data-selected={otherOn ? "true" : undefined}
                          onClick={(e) => {
                            e.preventDefault();
                            if (!active) {
                              return;
                            }
                            selectOther(q.id);
                          }}
                          onKeyDown={(e) => {
                            if (!active) {
                              return;
                            }
                            if (e.target !== e.currentTarget) {
                              return;
                            }
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              selectOther(q.id);
                            }
                          }}
                          role="radio"
                          tabIndex={active ? 0 : -1}
                        >
                          <span aria-hidden className={styles.key}>
                            {otherLetter}
                          </span>
                          <input
                            aria-label={`Custom answer for: ${q.prompt}`}
                            className={styles.optionInput}
                            onChange={(e) => {
                              if (!active) {
                                return;
                              }
                              updateCustom(q.id, e.target.value);
                            }}
                            onClick={(e) => {
                              e.stopPropagation();
                              if (!active) {
                                return;
                              }
                              selectOther(q.id);
                            }}
                            onKeyDown={(e) => {
                              e.stopPropagation();
                              if (!active) {
                                return;
                              }
                              if (e.key === "Enter") {
                                e.preventDefault();
                                commitCustom(q.id, e.currentTarget.value);
                              }
                            }}
                            placeholder="Something else…"
                            ref={(el) => {
                              customInputRefs.current[q.id] = el;
                            }}
                            tabIndex={active && otherOn ? 0 : -1}
                            type="text"
                            value={draft}
                          />
                        </div>
                      );
                    })()}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {variant === "command" && (
        <div className={styles.cmdBlock}>
          <div className={styles.cwd}>{cwd}</div>
          <pre className={styles.cmd}>{command}</pre>
        </div>
      )}

      {variant === "plan" && (
        <>
          <div className={styles.planIntro}>
            <div className={styles.planHeadline}>{resolvedPlanTitle}</div>
            <div className={styles.planSummary}>{resolvedPlanSummary}</div>
          </div>
          <div className={styles.todoWell}>
            <div className={styles.todoHead}>
              <span className={styles.todoHeadIcon}>
                <ListChecks
                  aria-hidden
                  className={styles.todoListIcon}
                  strokeWidth={2}
                />
              </span>
              <span className={styles.todoTitle}>To-dos</span>
              <span className={styles.todoCount}>{plan.length}</span>
            </div>
            <ul className={styles.todoList}>
              {planPreview.map((stepItem) => (
                <li className={styles.todoItem} key={stepItem.id}>
                  <span className={styles.todoIconWrap}>
                    <TodoDashedIcon />
                  </span>
                  <span className={styles.todoLabel}>{stepItem.title}</span>
                </li>
              ))}
            </ul>
            {hasPlanMore && (
              <>
                <div
                  className={`${styles.todoCollapsible}${
                    showPlanRest ? "" : ` ${styles.todoCollapsed}`
                  }`}
                >
                  <div className={styles.todoInner}>
                    <div className={styles.todoRest}>
                      <ul
                        className={`${styles.todoList} ${styles.todoListFlush}`}
                      >
                        {planRest.map((stepItem) => (
                          <li className={styles.todoItem} key={stepItem.id}>
                            <span className={styles.todoIconWrap}>
                              <TodoDashedIcon />
                            </span>
                            <span className={styles.todoLabel}>
                              {stepItem.title}
                            </span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  </div>
                </div>
                <button
                  aria-expanded={planExpanded}
                  className={styles.todoMore}
                  onClick={(e) => {
                    e.preventDefault();
                    setPlanExpanded((open) => !open);
                  }}
                  type="button"
                >
                  <span aria-hidden className={styles.todoMoreIcon}>
                    <svg
                      aria-hidden
                      className={styles.todoMoreGlyph}
                      viewBox="0 0 24 24"
                    >
                      {planExpanded ? (
                        <rect
                          fill="currentColor"
                          height="1.5"
                          rx="0.75"
                          width="14.5"
                          x="4.75"
                          y="11.25"
                        />
                      ) : (
                        <>
                          <circle cx="6" cy="12" fill="currentColor" r="1.25" />
                          <circle
                            cx="12"
                            cy="12"
                            fill="currentColor"
                            r="1.25"
                          />
                          <circle
                            cx="18"
                            cy="12"
                            fill="currentColor"
                            r="1.25"
                          />
                        </>
                      )}
                    </svg>
                  </span>
                  {planExpanded ? "Show less" : `${planRest.length} more`}
                </button>
              </>
            )}
          </div>
        </>
      )}

      <div className={styles.actions}>
        {variant === "questions" ? (
          <div
            aria-label={`Question ${safeStep + 1} of ${questions.length}`}
            className={styles.stepNav}
          >
            <button
              aria-label="Previous question"
              className={styles.stepArrow}
              disabled={safeStep <= 0}
              onClick={(e) => {
                e.preventDefault();
                goToStep(safeStep - 1);
              }}
              type="button"
            >
              <CaretUp
                aria-hidden
                className={styles.stepArrowIcon}
                strokeWidth={2}
              />
            </button>
            <span aria-live="polite" className={styles.stepBadge}>
              <RollingDigits value={stepLabel} />
            </span>
            <button
              aria-label="Next question"
              className={styles.stepArrow}
              disabled={safeStep >= questions.length - 1}
              onClick={(e) => {
                e.preventDefault();
                goToStep(safeStep + 1);
              }}
              type="button"
            >
              <CaretDown
                aria-hidden
                className={styles.stepArrowIcon}
                strokeWidth={2}
              />
            </button>
          </div>
        ) : variant === "plan" && autoUI !== "gone" ? (
          <div
            aria-label={`Auto approve in ${autoSecs} seconds`}
            aria-live="polite"
            className={`${styles.autoApprove}${
              autoUI === "leaving" ? ` ${styles.autoApproveOut}` : ""
            }`}
          >
            <span className={styles.autoApproveTip}>
              <button
                aria-label="Cancel auto approve"
                className={styles.autoApproveCancel}
                disabled={autoUI !== "active"}
                onClick={(e) => {
                  e.preventDefault();
                  cancelAutoApprove();
                }}
                type="button"
              >
                <svg
                  aria-hidden
                  className={styles.autoApprovePie}
                  height="16"
                  viewBox="0 0 24 24"
                  width="16"
                >
                  <circle
                    className={styles.autoApprovePieTrack}
                    cx="12"
                    cy="12"
                    fill="none"
                    r="9"
                    strokeWidth="1.8"
                  />
                  <circle
                    className={styles.autoApprovePieFill}
                    cx="12"
                    cy="12"
                    fill="none"
                    pathLength={1}
                    r="9"
                    strokeDasharray={1}
                    strokeLinecap="round"
                    strokeWidth="1.8"
                    style={{
                      strokeDashoffset: 1 - (autoTotal - autoSecs) / autoTotal,
                    }}
                    transform="rotate(-90 12 12)"
                  />
                </svg>
                <span aria-hidden className={styles.autoApproveCancelGlyph}>
                  <X size={8} strokeWidth={2.5} />
                </span>
              </button>
            </span>
            <span className={styles.autoApproveLabel}>
              Auto Approve in{" "}
              <span className={styles.autoApproveSecs}>
                <RollingDigits value={String(autoSecs)} />
              </span>
              s
            </span>
          </div>
        ) : (
          <span aria-hidden className={styles.actionsSpacer} />
        )}
        <div className={styles.actionBtns}>
          <button
            className={styles.btnGhost}
            onClick={(e) => {
              e.preventDefault();
              handleReject();
            }}
            type="button"
          >
            {resolvedReject}
          </button>
          <button
            className={styles.btnPrimary}
            disabled={!canContinue}
            onClick={(e) => {
              e.preventDefault();
              handleApprove();
            }}
            type="button"
          >
            {resolvedApprove}
            <ArrowElbowDownLeft
              aria-hidden
              className={styles.btnSubmitIcon}
              size={12}
              strokeWidth={2}
            />
          </button>
        </div>
      </div>
    </div>
  );
}
