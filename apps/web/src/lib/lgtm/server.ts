import { createServerFn } from '@tanstack/react-start'
import { env } from 'cloudflare:workers'
import type { RunnerStatus, Stats, Task, TaskDetail } from './types'

// wrangler.jsonc declares no `vars`, so worker-configuration.d.ts's generated
// Env type doesn't know these two — they exist only via .dev.vars / secrets.
const lgtmEnv = env as unknown as { LGTM_ORCHESTRATOR?: string; LGTM_TOKEN?: string }

async function api<T>(path: string): Promise<T> {
  const { LGTM_ORCHESTRATOR, LGTM_TOKEN } = lgtmEnv
  if (!LGTM_ORCHESTRATOR || !LGTM_TOKEN) {
    throw new Error('LGTM_ORCHESTRATOR and LGTM_TOKEN must be set in .dev.vars')
  }

  const base = LGTM_ORCHESTRATOR.replace(/\/+$/, '')
  const res = await fetch(`${base}/api${path}`, {
    headers: { Authorization: `Bearer ${LGTM_TOKEN}` },
  })
  if (!res.ok) {
    throw new Error(`orchestrator ${res.status} on ${path}`)
  }
  return res.json() as Promise<T>
}

export const getRunners = createServerFn({ method: 'GET' }).handler(
  async (): Promise<RunnerStatus[]> => api<RunnerStatus[]>('/runners'),
)

export const getTasks = createServerFn({ method: 'GET' }).handler(
  async (): Promise<Task[]> => api<Task[]>('/tasks'),
)

export const getStats = createServerFn({ method: 'GET' }).handler(
  async (): Promise<Stats> => api<Stats>('/stats'),
)

export const getTask = createServerFn({ method: 'GET' })
  .validator((id: string) => id)
  .handler(async ({ data }): Promise<TaskDetail> => api<TaskDetail>(`/tasks/${data}`))
