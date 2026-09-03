import { env } from "cloudflare:workers";
import { createServerFn } from "@tanstack/react-start";
import type {
  ActivityEntry,
  Memory,
  Project,
  RunnerStatus,
  Scratchpad,
  Session,
  Stats,
  Task,
  TaskDetail,
  Todo,
  TodoComment,
  TodoDetail,
  TodoPriority,
  TodoStatus,
} from "./types";

// wrangler.jsonc declares no `vars`, so worker-configuration.d.ts's generated
// Env type doesn't know these two — they exist only via .dev.vars / secrets.
const lgtmEnv = env as unknown as {
  LGTM_ORCHESTRATOR?: string;
  LGTM_TOKEN?: string;
};

async function api<T>(
  path: string,
  init?: { method?: "POST" | "PATCH" | "DELETE"; body?: unknown }
): Promise<T> {
  const { LGTM_ORCHESTRATOR, LGTM_TOKEN } = lgtmEnv;
  if (!(LGTM_ORCHESTRATOR && LGTM_TOKEN)) {
    throw new Error(
      "LGTM_ORCHESTRATOR and LGTM_TOKEN must be set in .dev.vars"
    );
  }

  const base = LGTM_ORCHESTRATOR.replace(/\/+$/, "");
  const headers: Record<string, string> = {
    Authorization: `Bearer ${LGTM_TOKEN}`,
  };
  if (init?.body !== undefined) {
    headers["content-type"] = "application/json";
  }

  const res = await fetch(`${base}/api${path}`, {
    body: init?.body === undefined ? undefined : JSON.stringify(init.body),
    headers,
    method: init?.method ?? "GET",
  });
  if (!res.ok) {
    // A refused mutation carries its reason in the body ("checks failed", "no
    // blocking findings cleared"). That reason is the only thing the reviewer
    // can act on, so it has to survive the throw.
    const reason = (await res.text()).trim();
    throw new Error(
      `orchestrator ${res.status} on ${path}${reason ? `: ${reason}` : ""}`
    );
  }
  // DELETE (and some PATCH) responses come back with an empty body, and
  // res.json() throws on that — read as text first so callers typed <void>
  // get undefined instead of a crash.
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

export const getRunners = createServerFn({ method: "GET" }).handler(
  async (): Promise<RunnerStatus[]> => api<RunnerStatus[]>("/runners")
);

export const getTasks = createServerFn({ method: "GET" }).handler(
  async (): Promise<Task[]> => api<Task[]>("/tasks")
);

export const getStats = createServerFn({ method: "GET" }).handler(
  async (): Promise<Stats> => api<Stats>("/stats")
);

export const getMemories = createServerFn({ method: "GET" }).handler(
  async (): Promise<Memory[]> => api<Memory[]>("/memories")
);

export const getTodos = createServerFn({ method: "GET" }).handler(
  async (): Promise<Todo[]> => api<Todo[]>("/todos")
);

export const getProjects = createServerFn({ method: "GET" }).handler(
  async (): Promise<Project[]> => api<Project[]>("/projects")
);

export const getSessions = createServerFn({ method: "GET" }).handler(
  async (): Promise<Session[]> => api<Session[]>("/sessions")
);

export const getActivity = createServerFn({ method: "GET" }).handler(
  async (): Promise<ActivityEntry[]> => api<ActivityEntry[]>("/activity")
);

export const getTask = createServerFn({ method: "GET" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<TaskDetail> => api<TaskDetail>(`/tasks/${data}`)
  );

export const approveTask = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>(`/tasks/${data}/approve`, { method: "POST" })
  );

export const rejectTask = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>(`/tasks/${data}/reject`, { method: "POST" })
  );

// An empty retry body means "same runner, same executor" to the orchestrator.
export const retryTask = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>(`/tasks/${data}/retry`, { body: {}, method: "POST" })
  );

export const sendFollowUp = createServerFn({ method: "POST" })
  .validator((input: { id: string; text: string }) => input)
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>(`/tasks/${data.id}/message`, {
        body: { text: data.text },
        method: "POST",
      })
  );

export const getTodo = createServerFn({ method: "GET" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<TodoDetail> => api<TodoDetail>(`/todos/${data}`)
  );

export const getScratchpads = createServerFn({ method: "GET" }).handler(
  async (): Promise<Scratchpad[]> => api<Scratchpad[]>("/scratchpads")
);

export const getScratchpad = createServerFn({ method: "GET" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<Scratchpad> =>
      api<Scratchpad>(`/scratchpads/${data}`)
  );

export const updateMemory = createServerFn({ method: "POST" })
  .validator((input: { id: string; content: string }) => input)
  .handler(
    async ({ data }): Promise<Memory> =>
      api<Memory>(`/memories/${data.id}`, {
        body: { content: data.content },
        method: "PATCH",
      })
  );

export const deleteMemory = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<void> =>
      api<void>(`/memories/${data}`, { method: "DELETE" })
  );

export const approveMemory = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<Memory> =>
      api<Memory>(`/memories/${data}/approve`, { method: "POST" })
  );

export const updateTodo = createServerFn({ method: "POST" })
  .validator(
    (input: {
      id: string;
      patch: {
        title?: string;
        description?: string;
        status?: TodoStatus;
        priority?: TodoPriority;
        assignee?: string | null;
        blockers?: string[];
        tags?: string[];
      };
    }) => input
  )
  .handler(
    async ({ data }): Promise<Todo> =>
      api<Todo>(`/todos/${data.id}`, { body: data.patch, method: "PATCH" })
  );

export const commentOnTodo = createServerFn({ method: "POST" })
  .validator((input: { id: string; body: string }) => input)
  .handler(
    async ({ data }): Promise<TodoComment> =>
      api<TodoComment>(`/todos/${data.id}/comments`, {
        body: { body: data.body },
        method: "POST",
      })
  );

export const createScratchpad = createServerFn({ method: "POST" })
  .validator((input: { repository?: string; content: string }) => input)
  .handler(
    async ({ data }): Promise<Scratchpad> =>
      api<Scratchpad>("/scratchpads", { body: data, method: "POST" })
  );

export const updateScratchpad = createServerFn({ method: "POST" })
  .validator(
    (input: {
      id: string;
      content?: string;
      archived?: boolean;
      tags?: string[];
    }) => input
  )
  .handler(
    async ({ data }): Promise<Scratchpad> =>
      api<Scratchpad>(`/scratchpads/${data.id}`, {
        // JSON.stringify drops undefined members on its own, so an omitted
        // field never reaches the orchestrator as an explicit null.
        body: {
          archived: data.archived,
          content: data.content,
          tags: data.tags,
        },
        method: "PATCH",
      })
  );

export const deleteScratchpad = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<void> =>
      api<void>(`/scratchpads/${data}`, { method: "DELETE" })
  );
