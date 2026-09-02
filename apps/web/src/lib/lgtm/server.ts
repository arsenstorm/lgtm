import { createServerFn } from '@tanstack/react-start'
import { env } from 'cloudflare:workers'
import type { RunnerStatus, Stats, Task, TaskDetail } from './types'

// wrangler.jsonc declares no `vars`, so worker-configuration.d.ts's generated
// Env type doesn't know these two — they exist only via .dev.vars / secrets.
const lgtmEnv = env as unknown as { LGTM_ORCHESTRATOR?: string; LGTM_TOKEN?: string }

async function api<T>(path: string, init?: { method?: 'POST'; body?: unknown }): Promise<T> {
  const { LGTM_ORCHESTRATOR, LGTM_TOKEN } = lgtmEnv
  if (!LGTM_ORCHESTRATOR || !LGTM_TOKEN) {
    throw new Error('LGTM_ORCHESTRATOR and LGTM_TOKEN must be set in .dev.vars')
  }

  const base = LGTM_ORCHESTRATOR.replace(/\/+$/, '')
  const headers: Record<string, string> = { Authorization: `Bearer ${LGTM_TOKEN}` }
  if (init?.body !== undefined) {
    headers['content-type'] = 'application/json'
  }

  const res = await fetch(`${base}/api${path}`, {
    method: init?.method ?? 'GET',
    headers,
    body: init?.body === undefined ? undefined : JSON.stringify(init.body),
  })
  if (!res.ok) {
    // A refused mutation carries its reason in the body ("checks failed", "no
    // blocking findings cleared"). That reason is the only thing the reviewer
    // can act on, so it has to survive the throw.
    const reason = (await res.text()).trim()
    throw new Error(`orchestrator ${res.status} on ${path}${reason ? `: ${reason}` : ''}`)
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

export const approveTask = createServerFn({ method: 'POST' })
  .validator((id: string) => id)
  .handler(async ({ data }): Promise<Task> => api<Task>(`/tasks/${data}/approve`, { method: 'POST' }))

export const rejectTask = createServerFn({ method: 'POST' })
  .validator((id: string) => id)
  .handler(async ({ data }): Promise<Task> => api<Task>(`/tasks/${data}/reject`, { method: 'POST' }))

// An empty retry body means "same runner, same executor" to the orchestrator.
export const retryTask = createServerFn({ method: 'POST' })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<Task> => api<Task>(`/tasks/${data}/retry`, { method: 'POST', body: {} }),
  )

export const sendFollowUp = createServerFn({ method: 'POST' })
  .validator((input: { id: string; text: string }) => input)
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>(`/tasks/${data.id}/message`, { method: 'POST', body: { text: data.text } }),
  )
