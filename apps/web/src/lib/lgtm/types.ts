export type TaskStatus =
  | 'queued' | 'running' | 'awaiting_review' | 'changes_requested'
  | 'conflicted' | 'approved' | 'merged' | 'rejected' | 'failed'
  | 'timed_out' | 'runner_lost' | 'cancelled'

export type Executor = 'claude' | 'codex'
export type SandboxProfile = 'off' | 'standard' | 'strict' | 'custom'
export type ExecutionStatus = 'running' | 'completed' | 'failed' | 'cancelled'

export interface RunnerInfo {
  name: string
  os: string
  arch: string
  executors: Executor[]
  slots: number
  ephemeral: boolean
  capabilities: string[]
  cpu_cores: number
  memory_mb: number
}

export interface RunnerStatus {
  info: RunnerInfo
  running: string[]
}

export interface ValidationResult {
  name: string
  command: string
  ok: boolean
  output_tail: string
}

export interface Finding {
  severity: 'blocking' | 'warning'
  file: string
  line: number | null
  message: string
}

export interface Review {
  findings: Finding[]
  executor: Executor | null
}

export interface Execution {
  attempt: number
  runner: string
  executor: Executor
  model: string | null
  started_at: number
  finished_at: number | null
  status: ExecutionStatus
  error: string | null
  cost_usd: number
  validation: ValidationResult[]
  artefacts: string[]
}

export interface TaskResult {
  branch: string
  diff: string
  changed_files: string[]
  validation: ValidationResult[]
  review: Review | null
  cost_usd: number
}

export interface TaskSpec {
  repository: string
  base_branch: string
  prompt: string
  executor: Executor
  runner: string | null
  kind: string
  sandbox: SandboxProfile | null
  model: string | null
  created_by: string | null
}

export interface Task {
  id: string
  spec: TaskSpec
  status: TaskStatus
  runner: string | null
  created_at: number
  result: TaskResult | null
  error: string | null
  executions: Execution[]
  scratchpad: string
  files: string[]
}

export interface Overlap {
  task: string
  files: string[]
}

/** Anything `res.json()` can hand back. Stating it explicitly lets the
 * serializability check on `getTask` stay on. */
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue }

export interface StoredEvent {
  at: number
  event: { type: string; [key: string]: JsonValue }
}

export interface TaskDetail {
  task: Task
  events: StoredEvent[]
  overlaps: Overlap[]
}

export interface ExecutorStats {
  executor: Executor
  attempts: number
  completed: number
  failed: number
}

export interface RunnerStats {
  runner: string
  attempts: number
  failed: number
  median_ms: number
}

export interface Stats {
  since: number
  tasks: number
  queued: number
  running: number
  awaiting_review: number
  approved: number
  merged: number
  failed: number
  cancelled: number
  rejected: number
  median_execution_ms: number
  median_queue_ms: number
  retried_tasks: number
  cost_usd: number
  by_executor: ExecutorStats[]
  by_runner: RunnerStats[]
  budget_daily_usd: number | null
  spent_today: number
}

export type MemorySource = 'user' | 'agent'
export type MemoryVerification = 'agent_proposed' | 'user_approved'

export interface Memory {
  id: string
  /** Null applies to every repository. */
  repository: string | null
  content: string
  created_at: number
  source: MemorySource
  verification: MemoryVerification
  proposed_by: string | null
  workspace: string | null
  created_by: string | null
}

export type TodoStatus = 'open' | 'in_progress' | 'done'
export type TodoPriority = 'low' | 'medium' | 'high'

export interface Todo {
  id: string
  repository: string | null
  title: string
  description: string
  status: TodoStatus
  created_at: number
  task: string | null
  priority: TodoPriority
  assignee: string | null
  blockers: string[]
  workspace: string | null
  created_by: string | null
}

export interface Session {
  id: string
  repository: string
  base_branch: string
  /** The first message cut to 60 chars; empty until one is sent. */
  title: string
  created_at: number
  workspace: string | null
  created_by: string | null
  archived: boolean
}

export interface ActivityEntry {
  at: number
  task: string
  owner: string
  repository: string
  event: string
  detail: string
}
