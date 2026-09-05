export type TaskStatus =
  | "queued"
  | "running"
  | "awaiting_review"
  | "changes_requested"
  | "conflicted"
  | "approved"
  | "merged"
  | "rejected"
  | "failed"
  | "timed_out"
  | "runner_lost"
  | "cancelled";

export type Executor = "claude" | "codex";
export type ReasoningEffort = "low" | "medium" | "high";
export type SandboxProfile = "off" | "standard" | "strict" | "custom";
export type ExecutionStatus = "running" | "completed" | "failed" | "cancelled";

export interface RunnerInfo {
  arch: string;
  capabilities: string[];
  cpu_cores: number;
  ephemeral: boolean;
  executors: Executor[];
  memory_mb: number;
  name: string;
  os: string;
  slots: number;
}

export interface RunnerStatus {
  info: RunnerInfo;
  running: string[];
}

export interface ValidationResult {
  command: string;
  name: string;
  ok: boolean;
  output_tail: string;
}

export interface Finding {
  file: string;
  line: number | null;
  message: string;
  severity: "blocking" | "warning";
}

export interface Review {
  executor: Executor | null;
  findings: Finding[];
}

export interface Execution {
  artefacts: string[];
  attempt: number;
  cost_usd: number;
  error: string | null;
  executor: Executor;
  finished_at: number | null;
  model: string | null;
  runner: string;
  skills: SkillRef[];
  started_at: number;
  status: ExecutionStatus;
  validation: ValidationResult[];
}

export interface TaskResult {
  branch: string;
  changed_files: string[];
  cost_usd: number;
  diff: string;
  review: Review | null;
  validation: ValidationResult[];
}

export interface TaskSpec {
  base_branch: string;
  created_by: string | null;
  executor: Executor;
  kind: string;
  model: string | null;
  prompt: string;
  reasoning_effort: ReasoningEffort | null;
  repository: string;
  runner: string | null;
  sandbox: SandboxProfile | null;
}

/** What the web composer sends to `POST /tasks`; serde defaults fill the rest
 * of `TaskSpec` on the orchestrator. */
export interface NewTaskSpec {
  base_branch: string;
  executor: Executor;
  model?: string | null;
  prompt: string;
  reasoning_effort?: ReasoningEffort | null;
  repository: string;
  runner?: string | null;
  sandbox?: SandboxProfile | null;
}

export interface Task {
  archived: boolean;
  created_at: number;
  error: string | null;
  executions: Execution[];
  files: string[];
  id: string;
  result: TaskResult | null;
  runner: string | null;
  scratchpad: string;
  spec: TaskSpec;
  status: TaskStatus;
  /** Model-written name; null until the inference lane answers. */
  title: string | null;
}

export interface Overlap {
  files: string[];
  task: string;
}

/** Anything `res.json()` can hand back. Stating it explicitly lets the
 * serializability check on `getTask` stay on. */
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface StoredEvent {
  at: number;
  event: { type: string; [key: string]: JsonValue };
}

export interface TaskDetail {
  events: StoredEvent[];
  overlaps: Overlap[];
  task: Task;
}

export interface ExecutorStats {
  attempts: number;
  completed: number;
  executor: Executor;
  failed: number;
}

export interface RunnerStats {
  attempts: number;
  failed: number;
  median_ms: number;
  runner: string;
}

export interface Stats {
  approved: number;
  awaiting_review: number;
  budget_daily_usd: number | null;
  by_executor: ExecutorStats[];
  by_runner: RunnerStats[];
  cancelled: number;
  cost_usd: number;
  failed: number;
  median_execution_ms: number;
  median_queue_ms: number;
  merged: number;
  queued: number;
  rejected: number;
  retried_tasks: number;
  running: number;
  since: number;
  spent_today: number;
  tasks: number;
}

export type MemorySource = "user" | "agent";
export type MemoryVerification = "agent_proposed" | "user_approved";

export interface Memory {
  content: string;
  created_at: number;
  created_by: string | null;
  id: string;
  proposed_by: string | null;
  /** Null applies to every repository. */
  repository: string | null;
  source: MemorySource;
  verification: MemoryVerification;
  workspace: string | null;
}

export interface SkillFile {
  content: string;
  path: string;
}

export interface Skill {
  content: string;
  created_at: number;
  created_by: string | null;
  description: string;
  files: SkillFile[];
  id: string;
  name: string;
  proposed_by: string | null;
  /** Null applies to every repository. */
  repository: string | null;
  revision: number;
  source: MemorySource;
  updated_at: number;
  verification: MemoryVerification;
  workspace: string | null;
}

export interface SkillRef {
  name: string;
  revision: number;
}

export interface Project {
  id: string;
  name: string;
  /** The next todo sequence number this project will hand out. */
  next_number: number;
  /** Prefixes every todo's display id, e.g. "L" in "L-3". */
  prefix: string;
  repository: string | null;
}

export type TodoStatus = "open" | "in_progress" | "done";
export type TodoPriority = "low" | "medium" | "high";

export interface Todo {
  assignee: string | null;
  blockers: string[];
  created_at: number;
  created_by: string | null;
  description: string;
  /** Server-computed: the project prefix plus `number`, e.g. "L-3". */
  display_id: string;
  id: string;
  number: number;
  priority: TodoPriority;
  repository: string | null;
  status: TodoStatus;
  tags: string[];
  task: string | null;
  title: string;
  workspace: string | null;
}

export interface TodoComment {
  /** The user who wrote it; null for the shared token or automation. */
  author: string | null;
  body: string;
  created_at: number;
  id: string;
  todo: string;
}

export interface TodoDetail {
  comments: TodoComment[];
  todo: Todo;
}

export interface Scratchpad {
  archived: boolean;
  content: string;
  created_at: number;
  created_by: string | null;
  id: string;
  /** Null is not tied to a repository. */
  repository: string | null;
  tags: string[];
  /** Named by whoever made it; the content never names the document. */
  title: string;
  /** Bumped only when content changes. */
  updated_at: number;
  workspace: string | null;
}

export interface Session {
  archived: boolean;
  base_branch: string;
  created_at: number;
  created_by: string | null;
  id: string;
  repository: string;
  /** The first message cut to 60 chars; empty until one is sent. */
  title: string;
  workspace: string | null;
}

export interface ActivityEntry {
  at: number;
  detail: string;
  event: string;
  owner: string;
  repository: string;
  task: string;
}

export type ChatRole = "person" | "agent";

export interface ChatStep {
  detail: string;
  tool: string;
}

/** One side of one exchange. An agent turn carries what the screen draws
 * around its prose; a failed turn's text is the reason. */
export interface ChatTurn {
  at: number;
  failed: boolean;
  refs: string[];
  role: ChatRole;
  steps: ChatStep[];
  text: string;
  worked_ms: number;
}

/** A conversation with the read-only workspace agent. Nothing in it changes
 * state. */
export interface Chat {
  archived: boolean;
  created_at: number;
  created_by: string | null;
  id: string;
  title: string;
  turns: ChatTurn[];
  workspace: string | null;
}
