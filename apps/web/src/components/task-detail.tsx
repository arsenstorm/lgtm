import {
  CheckCircle,
  FileCode,
  FileText,
  GitBranch,
  ShieldSlash,
  Stack,
  Warning,
  WarningCircle,
  XCircle,
} from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { DiffView } from "@/components/diff-view";
import { TaskActions } from "@/components/task-actions";
import { TimeAgo } from "@/components/time-ago";
import type {
  Finding,
  Overlap,
  Task,
  TaskDetail,
  TaskStatus,
  ValidationResult,
} from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export function TaskDetailView({ detail }: { detail: TaskDetail }) {
  const { task, overlaps } = detail;
  const result = task.result;
  const findings = result?.review ? sortFindings(result.review.findings) : [];
  const checks = result ? sortChecks(result.validation) : [];

  return (
    <article className="mx-auto flex w-full max-w-6xl flex-col gap-10 px-4 py-8 sm:px-6 lg:px-8">
      <TaskHeader task={task} />

      {task.error ? <ErrorPanel error={task.error} /> : null}
      {overlaps.length > 0 ? <OverlapPanel overlaps={overlaps} /> : null}

      {/* Below the two panels that qualify a decision, above the evidence that
          supports one: nothing here can be acted on before its warning is read. */}
      <TaskActions task={task} />

      {findings.length > 0 ? (
        <Section count={findings.length} title="Review">
          <ul className="divide-y rounded-lg border">
            {findings.map((finding, index) => (
              <FindingRow
                finding={finding}
                key={`${finding.file}:${finding.line}:${index}`}
              />
            ))}
          </ul>
        </Section>
      ) : null}

      {checks.length > 0 ? (
        <Section count={checks.length} title="Checks">
          <ul className="divide-y overflow-hidden rounded-lg border">
            {checks.map((check) => (
              <CheckRow check={check} key={check.name} />
            ))}
          </ul>
        </Section>
      ) : null}

      {result && result.changed_files.length > 0 ? (
        <Section count={result.changed_files.length} title="Changed files">
          <ul className="divide-y overflow-hidden rounded-lg border">
            {result.changed_files.map((file) => (
              <li className="flex items-start gap-2.5 px-3 py-2" key={file}>
                <FileCode className="mt-px size-3.5 shrink-0 text-muted-foreground" />
                <FilePath className="min-w-0 text-xs" path={file} />
              </li>
            ))}
          </ul>
        </Section>
      ) : null}

      <Section title="Diff">
        {result?.diff ? (
          <DiffView cacheKey={task.id} diff={result.diff} />
        ) : (
          <EmptyDiff status={task.status} />
        )}
      </Section>

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
    </article>
  );
}

function TaskHeader({ task }: { task: Task }) {
  const { spec } = task;

  return (
    <header className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <StatusPill status={task.status} />
        <span className="font-mono text-muted-foreground text-sm">
          {task.id}
        </span>
        <span aria-hidden className="text-muted-foreground/40">
          ·
        </span>
        <TimeAgo
          at={task.created_at}
          className="text-muted-foreground text-sm"
        />
      </div>

      <h1 className="max-w-[54ch] text-pretty font-medium text-lg leading-snug">
        {spec.prompt}
      </h1>

      <dl className="grid grid-cols-2 gap-x-6 gap-y-4 border-t pt-5 sm:grid-cols-3 lg:grid-cols-4">
        <Fact term="Repository">
          <RepositoryValue url={spec.repository} />
        </Fact>
        <Fact term="Base branch">
          <span className="inline-flex items-center gap-1.5">
            <GitBranch className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="font-mono">{spec.base_branch}</span>
          </span>
        </Fact>
        {task.result ? (
          <Fact term="Result branch">
            <span className="font-mono [overflow-wrap:anywhere]">
              {task.result.branch}
            </span>
          </Fact>
        ) : null}
        <Fact term="Runner">{task.runner ?? spec.runner ?? <Unset />}</Fact>
        <Fact term="Executor">{spec.executor}</Fact>
        <Fact term="Model">{spec.model ?? <Unset label="default" />}</Fact>
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
            {formatCost(task.result?.cost_usd ?? 0)}
          </span>
        </Fact>
      </dl>
    </header>
  );
}

function ErrorPanel({ error }: { error: string }) {
  return (
    <section className="rounded-lg border border-destructive/30 bg-destructive/5 p-4">
      <h2 className="flex items-center gap-2 font-medium text-destructive text-sm">
        <XCircle className="size-4 shrink-0" />
        This task failed
      </h2>
      <pre className="mt-3 whitespace-pre-wrap text-xs leading-relaxed [overflow-wrap:anywhere]">
        {error}
      </pre>
    </section>
  );
}

function OverlapPanel({ overlaps }: { overlaps: Overlap[] }) {
  return (
    <section className="rounded-lg border border-amber-500/35 bg-amber-500/5 p-4">
      <h2 className="flex items-center gap-2 font-medium text-amber-700 text-sm dark:text-amber-400">
        <Stack className="size-4 shrink-0" />
        {overlaps.length === 1
          ? "One other unmerged task touches these files"
          : `${overlaps.length} other unmerged tasks touch these files`}
      </h2>
      <p className="mt-1 max-w-[54ch] text-pretty text-muted-foreground text-sm">
        Merging this task may conflict with work that has not landed yet.
      </p>
      <ul className="mt-3 flex flex-col gap-2">
        {overlaps.map((overlap) => (
          <li
            className="flex flex-wrap items-baseline gap-x-3 gap-y-1"
            key={overlap.task}
          >
            <span className="font-mono text-amber-800 text-xs dark:text-amber-300">
              {overlap.task}
            </span>
            <span className="flex min-w-0 flex-wrap gap-x-3 gap-y-1">
              {overlap.files.map((file) => (
                <FilePath className="text-xs" key={file} path={file} />
              ))}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function FindingRow({ finding }: { finding: Finding }) {
  const blocking = finding.severity === "blocking";

  return (
    <li className={cn("flex gap-3 p-3", blocking && "bg-destructive/5")}>
      {blocking ? (
        <WarningCircle className="mt-0.5 size-4 shrink-0 text-destructive" />
      ) : (
        <Warning className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400" />
      )}
      <div className="flex min-w-0 flex-col gap-1">
        <p className="max-w-[54ch] text-pretty text-sm">{finding.message}</p>
        <p className="font-mono text-muted-foreground text-xs [overflow-wrap:anywhere]">
          {finding.file}
          {finding.line == null ? null : `:${finding.line}`}
        </p>
      </div>
    </li>
  );
}

function CheckRow({ check }: { check: ValidationResult }) {
  if (check.ok) {
    return (
      <li className="flex items-center gap-2.5 px-3 py-2">
        <CheckCircle className="size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
        <span className="font-medium text-sm">{check.name}</span>
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
        <XCircle className="size-4 shrink-0 text-destructive" />
        <span className="font-medium text-sm">{check.name}</span>
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
  title: string;
  count?: number;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-baseline gap-2">
        <h2 className="font-medium text-muted-foreground text-xs uppercase tracking-[0.08em]">
          {title}
        </h2>
        {count == null ? null : (
          <span className="text-muted-foreground/70 text-xs tabular-nums">
            {count}
          </span>
        )}
      </div>
      {children}
    </section>
  );
}

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <dt className="text-muted-foreground text-xs">{term}</dt>
      <dd className="font-medium text-sm">{children}</dd>
    </div>
  );
}

function Unset({ label = "—" }: { label?: string }) {
  return <span className="font-normal text-muted-foreground">{label}</span>;
}

function RepositoryValue({ url }: { url: string }) {
  const label = url.replace(/^https?:\/\//, "").replace(/\.git$/, "");
  if (!url.startsWith("http")) {
    return <span className="[overflow-wrap:anywhere]">{label}</span>;
  }
  return (
    <a
      className="underline decoration-muted-foreground/40 underline-offset-2 [overflow-wrap:anywhere] hover:decoration-current"
      href={url.replace(/\.git$/, "")}
      rel="noreferrer"
      target="_blank"
    >
      {label}
    </a>
  );
}

function FilePath({ path, className }: { path: string; className?: string }) {
  const cut = path.lastIndexOf("/") + 1;
  return (
    <span className={cn("font-mono [overflow-wrap:anywhere]", className)}>
      <span className="text-muted-foreground">{path.slice(0, cut)}</span>
      {path.slice(cut)}
    </span>
  );
}

const STATUS_TONE: Record<TaskStatus, string> = {
  approved:
    "border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400",
  awaiting_review:
    "border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  cancelled: "border-border bg-muted text-muted-foreground",
  changes_requested:
    "border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  conflicted:
    "border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  failed: "border-destructive/35 bg-destructive/10 text-destructive",
  merged:
    "border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400",
  queued: "border-border bg-muted text-muted-foreground",
  rejected: "border-destructive/35 bg-destructive/10 text-destructive",
  runner_lost: "border-destructive/35 bg-destructive/10 text-destructive",
  running: "border-sky-500/35 bg-sky-500/10 text-sky-700 dark:text-sky-300",
  timed_out: "border-destructive/35 bg-destructive/10 text-destructive",
};

function StatusPill({ status }: { status: TaskStatus }) {
  const words = status.replace(/_/g, " ");

  return (
    <span
      className={cn(
        "inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 font-medium text-xs",
        STATUS_TONE[status]
      )}
    >
      <span aria-hidden className="size-1.5 shrink-0 rounded-full bg-current" />
      <span className="inline-block first-letter:uppercase">{words}</span>
    </span>
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

function formatCost(usd: number) {
  return `$${usd.toFixed(2)}`;
}
