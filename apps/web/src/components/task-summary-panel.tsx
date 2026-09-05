import { FileCode, FileText, ShieldSlash } from "@phosphor-icons/react";
import type { ReactNode } from "react";

import { DiffView } from "@/components/diff-view";
import { FilePath } from "@/components/file-path";
import {
  CircleCheckIcon,
  CircleWarningIcon,
  CircleXIcon,
  CodeBranchIcon,
  WarningIcon,
} from "@/components/icons";
import type {
  Finding,
  TaskDetail,
  TaskStatus,
  ValidationResult,
} from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

const HTTP_PROTOCOL = /^https?:\/\//;
const DOT_GIT = /\.git$/;

/** The Codex-style pinned summary: everything a reviewer checks lives here so
 * the transcript stays a conversation. */
export function TaskSummaryPanel({
  detail,
  className,
}: {
  className?: string;
  detail: TaskDetail;
}) {
  const { task } = detail;
  const { result } = task;
  const findings = result?.review ? sortFindings(result.review.findings) : [];
  const checks = result ? sortChecks(result.validation) : [];

  return (
    <aside
      aria-label="Task summary"
      className={cn("flex min-w-0 flex-col gap-8", className)}
    >
      <Environment detail={detail} />

      <Section count={result?.changed_files.length} title="Changes">
        {result?.diff ? (
          <div className="flex min-w-0 flex-col gap-3">
            <DiffStat diff={result.diff} />
            <ul className="divide-y overflow-hidden rounded-lg border">
              {result.changed_files.map((file) => (
                <li className="flex items-start gap-2.5 px-3 py-2" key={file}>
                  <FileCode className="mt-px size-3.5 shrink-0 text-muted-foreground" />
                  <FilePath className="min-w-0 text-xs" path={file} />
                </li>
              ))}
            </ul>
            <DiffView cacheKey={task.id} diff={result.diff} />
          </div>
        ) : (
          <EmptyDiff status={task.status} />
        )}
      </Section>

      {checks.length > 0 ? (
        <Section count={checks.length} title="Checks">
          <ul className="divide-y overflow-hidden rounded-lg border">
            {checks.map((check) => (
              <CheckRow check={check} key={check.name} />
            ))}
          </ul>
        </Section>
      ) : null}

      {findings.length > 0 ? (
        <Section count={findings.length} title="Review">
          <ul className="divide-y rounded-lg border">
            {findings.map((finding) => (
              <FindingRow
                finding={finding}
                key={`${finding.file}:${finding.line}:${finding.message}`}
              />
            ))}
          </ul>
        </Section>
      ) : null}

      {task.scratchpad.trim() ? (
        <Section title="Agent scratchpad">
          <div className="rounded-lg border bg-muted/30 p-4">
            {/* The agent writes markdown; rendering it as-is keeps the exact
                bytes it committed readable without a parser in the way. */}
            <pre className="max-h-96 overflow-auto whitespace-pre-wrap text-xs leading-relaxed [overflow-wrap:anywhere]">
              {task.scratchpad.trim()}
            </pre>
          </div>
        </Section>
      ) : null}
    </aside>
  );
}

function Environment({ detail }: { detail: TaskDetail }) {
  const { task } = detail;
  const { spec } = task;
  // What the last attempt actually ran with; a task that never ran claims
  // nothing.
  const lastRun = task.executions.at(-1);

  return (
    <Section title="Environment">
      <dl className="grid grid-cols-2 gap-x-5 gap-y-4">
        <Fact className="col-span-2" term="Repository">
          <RepositoryValue url={spec.repository} />
        </Fact>
        <Fact term="Base branch">
          <span className="inline-flex items-center gap-1.5">
            <CodeBranchIcon className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="font-mono">{spec.base_branch}</span>
          </span>
        </Fact>
        {task.result ? (
          <Fact className="col-span-2" term="Result branch">
            <span className="break-words font-mono">{task.result.branch}</span>
          </Fact>
        ) : null}
        <Fact term="Runner">{task.runner ?? spec.runner ?? <Unset />}</Fact>
        <Fact term="Executor">{spec.executor}</Fact>
        <Fact term="Model">{spec.model ?? <Unset label="default" />}</Fact>
        {lastRun ? (
          <Fact className="col-span-2" term="Skills">
            {lastRun.skills.length === 0 ? (
              <Unset label="none" />
            ) : (
              lastRun.skills.map((skill, index) => (
                <span className="font-mono" key={skill.name}>
                  {index === 0 ? "" : ", "}
                  {skill.name}
                </span>
              ))
            )}
          </Fact>
        ) : null}
        <Fact term="Sandbox">
          {spec.sandbox === "off" ? (
            <span className="inline-flex items-center gap-1.5 text-amber-700 dark:text-amber-400">
              <ShieldSlash className="size-3.5 shrink-0" />
              off
            </span>
          ) : (
            (spec.sandbox ?? <Unset />)
          )}
        </Fact>
        <Fact term="Cost">
          <span className="tabular-nums">
            ${(task.result?.cost_usd ?? 0).toFixed(2)}
          </span>
        </Fact>
      </dl>
    </Section>
  );
}

const DIFF_LINE = /^[+-]/;
const DIFF_FILE_HEADER = /^(\+\+\+|---)/;

function DiffStat({ diff }: { diff: string }) {
  let added = 0;
  let removed = 0;
  for (const line of diff.split("\n")) {
    if (!DIFF_LINE.test(line) || DIFF_FILE_HEADER.test(line)) {
      continue;
    }
    if (line.startsWith("+")) {
      added += 1;
    } else {
      removed += 1;
    }
  }

  return (
    <p className="font-mono text-xs tabular-nums">
      <span className="text-emerald-700 dark:text-emerald-400">+{added}</span>{" "}
      <span className="text-destructive">−{removed}</span>
    </p>
  );
}

function FindingRow({ finding }: { finding: Finding }) {
  const blocking = finding.severity === "blocking";

  return (
    <li className={cn("flex gap-3 p-3", blocking && "bg-destructive/5")}>
      {blocking ? (
        <CircleWarningIcon className="mt-0.5 size-4 shrink-0 text-destructive" />
      ) : (
        <WarningIcon className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400" />
      )}
      <div className="flex min-w-0 flex-col gap-1">
        <p className="max-w-[54ch] text-pretty text-sm">{finding.message}</p>
        <p className="break-words font-mono text-muted-foreground text-xs">
          {finding.file}
          {finding.line === null ? null : `:${finding.line}`}
        </p>
      </div>
    </li>
  );
}

function CheckRow({ check }: { check: ValidationResult }) {
  if (check.ok) {
    return (
      <li className="flex items-center gap-2.5 px-3 py-2">
        <CircleCheckIcon className="size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
        <span className="truncate font-medium text-sm">{check.name}</span>
        <code
          className="min-w-0 truncate text-muted-foreground text-xs"
          title={check.command}
        >
          {check.command}
        </code>
      </li>
    );
  }

  return (
    <li className="bg-destructive/5 p-3">
      <div className="flex items-center gap-2.5">
        <CircleXIcon className="size-4 shrink-0 text-destructive" />
        <span className="truncate font-medium text-sm">{check.name}</span>
        <code
          className="min-w-0 truncate text-muted-foreground text-xs"
          title={check.command}
        >
          {check.command}
        </code>
      </div>
      {check.output_tail.trim() ? (
        <pre className="mt-3 max-h-80 overflow-auto rounded-md border bg-background p-3 text-xs leading-relaxed">
          {check.output_tail.trimEnd()}
        </pre>
      ) : null}
    </li>
  );
}

function EmptyDiff({ status }: { status: TaskStatus }) {
  const pending = status === "queued" || status === "running";

  return (
    <div className="flex min-h-40 flex-col items-center justify-center gap-2 rounded-lg border border-dashed p-8 text-center">
      <FileText className="size-5 text-muted-foreground" />
      <p className="font-medium text-sm">
        {pending ? "No diff yet" : "This task produced no diff"}
      </p>
      <p className="max-w-[40ch] text-pretty text-muted-foreground text-sm">
        {pending
          ? "The agent is still working. The diff appears here once the run finishes."
          : "The run ended before it wrote any changes."}
      </p>
    </div>
  );
}

function Section({
  title,
  count,
  children,
}: {
  children: ReactNode;
  count?: number;
  title: string;
}) {
  return (
    <section className="flex min-w-0 flex-col gap-3">
      <div className="flex items-baseline gap-2">
        <h3 className="truncate font-medium text-muted-foreground text-xs uppercase tracking-[0.08em]">
          {title}
        </h3>
        {count === undefined ? null : (
          <span className="text-muted-foreground/70 text-xs tabular-nums">
            {count}
          </span>
        )}
      </div>
      {children}
    </section>
  );
}

function Fact({
  term,
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
  term: string;
}) {
  return (
    <div className={cn("flex min-w-0 flex-col gap-1", className)}>
      <dt className="text-muted-foreground text-xs leading-none">{term}</dt>
      <dd className="font-medium text-sm leading-5">{children}</dd>
    </div>
  );
}

function Unset({ label = "—" }: { label?: string }) {
  return <span className="font-normal text-muted-foreground">{label}</span>;
}

function RepositoryValue({ url }: { url: string }) {
  const label = url.replace(HTTP_PROTOCOL, "").replace(DOT_GIT, "");
  if (!url.startsWith("http")) {
    return <span className="break-words">{label}</span>;
  }
  return (
    <a
      className="break-words underline decoration-from-font decoration-muted-foreground/40 underline-offset-2 [text-decoration-skip-ink:auto] [text-underline-position:from-font] hover:decoration-current"
      href={url.replace(DOT_GIT, "")}
      rel="noreferrer"
      target="_blank"
    >
      {label}
    </a>
  );
}

function sortFindings(findings: Finding[]) {
  return [...findings].sort(
    (a, b) =>
      Number(b.severity === "blocking") - Number(a.severity === "blocking")
  );
}

function sortChecks(checks: ValidationResult[]) {
  return [...checks].sort((a, b) => Number(a.ok) - Number(b.ok));
}
