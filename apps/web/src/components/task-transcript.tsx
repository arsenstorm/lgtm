import Markdown from "react-markdown";

import { TextResponse } from "@/components/aicss/TextResponse";
import { ThinkingState } from "@/components/aicss/ThinkingState";
import { FilePath } from "@/components/file-path";
import {
  ChevronIcon,
  CodeIcon,
  type IconComponent,
  ShieldAlertIcon,
  TerminalIcon,
} from "@/components/icons";
import { TimeAgo } from "@/components/time-ago";
import { Bubble, BubbleContent } from "@/components/ui/bubble";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import {
  Message,
  MessageContent,
  MessageFooter,
} from "@/components/ui/message";
import type { SkillRef, StoredEvent, Task } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

type EventBody = StoredEvent["event"];

const str = (event: EventBody, key: string): string =>
  typeof event[key] === "string" ? (event[key] as string) : "";

const strings = (event: EventBody, key: string): string[] => {
  const value = event[key];
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((item): item is string => typeof item === "string");
};

/** A runner from before skills existed sends none, and the event is stored as
 *  loose JSON, so every item is checked before it counts. */
const skillRefs = (event: EventBody): SkillRef[] => {
  const value = event.skills;
  if (!Array.isArray(value)) {
    return [];
  }
  const refs: SkillRef[] = [];
  for (const item of value) {
    if (
      item &&
      typeof item === "object" &&
      !Array.isArray(item) &&
      typeof item.name === "string" &&
      typeof item.revision === "number"
    ) {
      refs.push({ name: item.name, revision: item.revision });
    }
  }
  return refs;
};

const asNumber = (event: EventBody, key: string): number | null =>
  typeof event[key] === "number" ? (event[key] as number) : null;

type Tone = "muted" | "good" | "warn" | "bad";

type Item =
  | { kind: "user"; at: number; by: string | null; text: string }
  | { kind: "agent"; at: number; text: string }
  | {
      kind: "tool";
      at: number;
      tool: "command" | "file";
      icon: IconComponent;
      body: React.ReactNode;
      lines: string[];
    }
  | { kind: "output"; at: number; lines: string[] }
  | { kind: "permission"; at: number; what: string; reason: string }
  | { kind: "marker"; at: number; text: string; tone: Tone; boundary: boolean };

const marker = (at: number, text: string, tone: Tone = "muted"): Item => ({
  at,
  boundary: false,
  kind: "marker",
  text,
  tone,
});

/** A run boundary renders as a labeled separator; everything else stays an
 * inline status line. */
const boundary = (at: number, text: string, tone: Tone = "muted"): Item => ({
  at,
  boundary: true,
  kind: "marker",
  text,
  tone,
});

/** One stored event to one transcript item; null drops it from the feed. */
function toItem({ at, event }: StoredEvent): Item | null {
  switch (event.type) {
    case "message":
      return {
        at,
        by: str(event, "by") || null,
        kind: "user",
        text: str(event, "text"),
      };
    case "progress":
      return { at, kind: "agent", text: str(event, "text") };
    case "command":
      return {
        at,
        body: <span className="min-w-0">{str(event, "command")}</span>,
        icon: TerminalIcon,
        kind: "tool",
        lines: [],
        tool: "command",
      };
    case "file_changed":
      return {
        at,
        body: <FilePath path={str(event, "path")} />,
        icon: CodeIcon,
        kind: "tool",
        lines: [],
        tool: "file",
      };
    case "output":
      return { at, kind: "output", lines: [str(event, "line")] };
    case "permission_requested":
      return {
        at,
        kind: "permission",
        reason: str(event, "reason"),
        what: `${str(event, "kind")} ${str(event, "target")}`.trim(),
      };
    default:
      return lifecycleMarker(at, event);
  }
}

function lifecycleMarker(at: number, event: EventBody): Item | null {
  const model = str(event, "model");
  switch (event.type) {
    case "started": {
      const skills = skillRefs(event).length;
      const parts = ["Run started"];
      if (model) {
        parts.push(model);
      }
      if (skills > 0) {
        parts.push(`${skills} skill${skills === 1 ? "" : "s"}`);
      }
      return boundary(at, parts.join(" · "));
    }
    case "validating":
      return marker(
        at,
        `Running checks: ${strings(event, "names").join(", ")}`
      );
    case "completed":
      return boundary(at, "Run completed", "good");
    case "failed":
      return boundary(at, "Run failed", "bad");
    case "timed_out":
      return boundary(at, `Timed out after ${asNumber(event, "secs")}s`, "bad");
    case "runner_lost":
      return boundary(at, "Runner lost while the task was running", "bad");
    case "cancelled":
      return boundary(at, "Cancelled", "bad");
    case "retry":
      return boundary(
        at,
        `Retrying (attempt ${asNumber(event, "attempt")}): ${str(event, "reason")}`,
        "warn"
      );
    case "requeued":
      return boundary(
        at,
        `Requeued on ${str(event, "runner") || "any runner"} · ${str(event, "executor")}`
      );
    case "conflicted":
      return marker(
        at,
        `Rebase onto ${str(event, "base")} conflicted on ${strings(event, "files").length} files`,
        "warn"
      );
    case "network_denied":
      return marker(at, `Network denied: ${str(event, "host")}`, "warn");
    case "host_allowed":
      return marker(at, `Host allowed: ${str(event, "host")}`, "good");
    case "policy_decision":
      return policyMarker(at, event);
    case "orchestrated":
      return marker(
        at,
        `Orchestrator asked to ${str(event, "action")} — ${event.applied === true ? "applied" : "not applied"}`
      );
    case "auto_approved":
      return marker(at, "Approved by policy", "good");
    case "auto_merged":
      return marker(at, "Merged by policy", "good");
    case "pushed":
      return marker(at, `Pushed ${str(event, "branch")}`, "good");
    case "discarded":
      return marker(at, "Worktree and branch discarded");
    case "pr_reviewed":
      return marker(at, `Pull request review: ${str(event, "state")}`);
    case "scratchpad":
      return marker(at, "Updated the scratchpad");
    case "artefact":
      return marker(
        at,
        `Left artefact ${str(event, "name")} (${formatBytes(asNumber(event, "size") ?? 0)})`
      );
    default:
      return marker(at, event.type.replace(/_/g, " "));
  }
}

function policyMarker(at: number, event: EventBody): Item {
  const allowed = event.allowed === true;
  const reasons = strings(event, "reasons").join("; ");
  return marker(
    at,
    `Policy ${allowed ? "allowed" : "blocked"} ${str(event, "action")}${reasons ? `: ${reasons}` : ""}`,
    allowed ? "muted" : "warn"
  );
}

function formatBytes(size: number): string {
  if (size >= 1024 * 1024) {
    return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  }
  return size >= 1024 ? `${Math.round(size / 1024)} KB` : `${size} B`;
}

/** The prompt leads as the first user message, then every stored event in
 * arrival order, with consecutive output lines folded into one block. */
function buildItems(task: Task, events: StoredEvent[]): Item[] {
  const items: Item[] = [
    {
      at: task.created_at,
      by: task.spec.created_by,
      kind: "user",
      text: task.spec.prompt,
    },
  ];
  for (const stored of events) {
    const item = toItem(stored);
    if (!item) {
      continue;
    }
    const last = items.at(-1);
    // A command's output belongs to the command; the rest of the raw stream
    // (agent wire chatter) folds into its own block.
    if (item.kind === "output" && last?.kind === "tool") {
      last.lines.push(...item.lines);
    } else if (item.kind === "output" && last?.kind === "output") {
      last.lines.push(...item.lines);
    } else {
      items.push(item);
    }
  }
  return items;
}

type ToolItem = Extract<Item, { kind: "tool" }>;
type Grouped = Item | { kind: "activity"; at: number; tools: ToolItem[] };

/** Consecutive tool work collapses into one summarised row, so the default
 * read of the transcript is the agent's narration, not its every keystroke. */
function groupActivity(items: Item[]): Grouped[] {
  const grouped: Grouped[] = [];
  for (const item of items) {
    const last = grouped.at(-1);
    if (item.kind !== "tool") {
      grouped.push(item);
    } else if (last?.kind === "activity") {
      last.tools.push(item);
    } else if (last?.kind === "tool") {
      grouped[grouped.length - 1] = {
        at: last.at,
        kind: "activity",
        tools: [last, item],
      };
    } else {
      grouped.push(item);
    }
  }
  return grouped;
}

function activityLabel(tools: ToolItem[]): string {
  const commands = tools.filter((tool) => tool.tool === "command").length;
  const files = tools.length - commands;
  const parts: string[] = [];
  if (commands > 0) {
    parts.push(`Ran ${commands} ${commands === 1 ? "command" : "commands"}`);
  }
  if (files > 0) {
    parts.push(
      `${commands > 0 ? "touched" : "Touched"} ${files} ${files === 1 ? "file" : "files"}`
    );
  }
  return parts.join(" · ");
}

export function TaskTranscript({
  task,
  events,
}: {
  events: StoredEvent[];
  task: Task;
}) {
  const items = groupActivity(buildItems(task, events));
  const live = task.status === "queued" || task.status === "running";

  return (
    <ol className="flex min-w-0 flex-col gap-5" role="list">
      {items.map((item, index) => (
        // Stored events carry no id; the feed is append-only, so position is
        // stable for everything already rendered.
        // biome-ignore lint/suspicious/noArrayIndexKey: append-only feed
        <li className="flex min-w-0" key={index}>
          <TranscriptItem item={item} />
        </li>
      ))}
      {live ? (
        <li>
          <Marker>
            <MarkerContent>
              <ThinkingState
                label={
                  task.status === "queued" ? "Waiting for a runner" : "Working"
                }
              />
            </MarkerContent>
          </Marker>
        </li>
      ) : null}
    </ol>
  );
}

function TranscriptItem({ item }: { item: Grouped }) {
  switch (item.kind) {
    case "activity":
      return <ActivityGroup at={item.at} tools={item.tools} />;
    case "user":
      return <UserMessage at={item.at} by={item.by} text={item.text} />;
    case "agent":
      return (
        <Message>
          <MessageContent>
            <Bubble variant="ghost">
              <BubbleContent>
                <TextResponse>
                  <Markdown>{item.text}</Markdown>
                </TextResponse>
              </BubbleContent>
            </Bubble>
          </MessageContent>
        </Message>
      );
    case "tool":
      return <ToolRow body={item.body} icon={item.icon} lines={item.lines} />;
    case "output":
      return <OutputBlock lines={item.lines} />;
    case "permission":
      return <PermissionRow reason={item.reason} what={item.what} />;
    case "marker":
      return <StatusMarker item={item} />;
    default:
      return null;
  }
}

function UserMessage({
  text,
  by,
  at,
}: {
  at: number;
  by: string | null;
  text: string;
}) {
  return (
    <Message align="end">
      <MessageContent>
        <Bubble align="end" variant="muted">
          <BubbleContent>
            <p className="whitespace-pre-wrap [overflow-wrap:anywhere]">
              {text}
            </p>
          </BubbleContent>
        </Bubble>
        <MessageFooter className="gap-2">
          {by ? <span>{by}</span> : null}
          <TimeAgo at={at} />
        </MessageFooter>
      </MessageContent>
    </Message>
  );
}

function ActivityGroup({ tools, at }: { at: number; tools: ToolItem[] }) {
  return (
    <details className="group min-w-0 flex-1">
      <summary className="cursor-pointer list-none rounded-md [&::-webkit-details-marker]:hidden">
        <Marker>
          <MarkerIcon>
            <ChevronIcon className="transition-transform group-open:rotate-90" />
          </MarkerIcon>
          <MarkerContent className="transition-colors group-hover:text-foreground">
            {activityLabel(tools)}
            <TimeAgo
              at={at}
              className="ml-2 text-muted-foreground/70 text-xs"
            />
          </MarkerContent>
        </Marker>
      </summary>
      <div className="mt-3 ml-2 flex min-w-0 flex-col gap-3 border-l pl-4">
        {tools.map((tool, index) => (
          // Position is stable: the grouped run is rebuilt whole on refetch.
          // biome-ignore lint/suspicious/noArrayIndexKey: append-only feed
          <ToolRow
            body={tool.body}
            icon={tool.icon}
            key={index}
            lines={tool.lines}
          />
        ))}
      </div>
    </details>
  );
}

function ToolRow({
  icon: Glyph,
  body,
  lines,
}: {
  body: React.ReactNode;
  icon: IconComponent;
  lines: string[];
}) {
  const row = (
    <Marker className="items-start">
      <MarkerIcon className="mt-0.5">
        <Glyph />
      </MarkerIcon>
      <MarkerContent>
        <code className="whitespace-pre-wrap font-mono text-xs leading-5 [overflow-wrap:anywhere]">
          {body}
        </code>
        {lines.length > 0 ? (
          <span className="ml-2 whitespace-nowrap text-muted-foreground/60 text-xs">
            {lines.length} {lines.length === 1 ? "line" : "lines"}
          </span>
        ) : null}
      </MarkerContent>
    </Marker>
  );

  if (lines.length === 0) {
    return row;
  }

  return (
    <details className="group min-w-0 flex-1">
      <summary className="cursor-pointer list-none rounded-md transition-colors hover:bg-muted/50 [&::-webkit-details-marker]:hidden">
        {row}
      </summary>
      <pre className="mt-2 ml-6 max-h-72 overflow-auto rounded-lg border bg-muted/30 p-3 text-muted-foreground text-xs leading-relaxed">
        {lines.join("\n")}
      </pre>
    </details>
  );
}

function OutputBlock({ lines }: { lines: string[] }) {
  return (
    <details className="group min-w-0 flex-1">
      <summary className="w-fit cursor-pointer list-none text-muted-foreground text-xs hover:text-foreground [&::-webkit-details-marker]:hidden">
        <span className="group-open:hidden">
          Raw output · {lines.length} {lines.length === 1 ? "line" : "lines"}
        </span>
        <span className="hidden group-open:inline">Hide raw output</span>
      </summary>
      <pre className="mt-2 max-h-72 overflow-auto rounded-lg border bg-muted/30 p-3 text-xs leading-relaxed">
        {lines.join("\n")}
      </pre>
    </details>
  );
}

function PermissionRow({ what, reason }: { reason: string; what: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-1 rounded-lg border border-amber-500/35 bg-amber-500/5 p-3">
      <p className="flex items-center gap-2 font-medium text-amber-700 text-sm dark:text-amber-400">
        <ShieldAlertIcon className="size-4 shrink-0" />
        The sandbox refused {what}
      </p>
      {reason ? (
        <p className="max-w-[54ch] text-pretty text-muted-foreground text-sm">
          {reason}
        </p>
      ) : null}
      <p className="text-muted-foreground text-xs">
        A person can grant it with <code>lgtm allow</code>.
      </p>
    </div>
  );
}

const TONE_TEXT: Record<Tone, string> = {
  bad: "text-destructive",
  good: "text-emerald-700 dark:text-emerald-400",
  muted: "",
  warn: "text-amber-700 dark:text-amber-400",
};

function StatusMarker({ item }: { item: Extract<Item, { kind: "marker" }> }) {
  return (
    <Marker variant={item.boundary ? "separator" : "default"}>
      <MarkerContent className={cn(TONE_TEXT[item.tone])}>
        {item.text}
        <TimeAgo
          at={item.at}
          className="ml-2 text-muted-foreground/70 text-xs"
        />
      </MarkerContent>
    </Marker>
  );
}
