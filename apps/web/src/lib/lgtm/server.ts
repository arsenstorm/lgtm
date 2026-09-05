import { env } from "cloudflare:workers";
import { createServerFn } from "@tanstack/react-start";
import type {
  ActivityEntry,
  Chat,
  Executor,
  Memory,
  NewTaskSpec,
  Project,
  RunnerStatus,
  Scratchpad,
  Session,
  Skill,
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

const TRAILING_SLASHES = /\/+$/;

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

  const base = LGTM_ORCHESTRATOR.replace(TRAILING_SLASHES, "");
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

export const getSkills = createServerFn({ method: "GET" }).handler(
  async (): Promise<Skill[]> => api<Skill[]>("/skills")
);

// The orchestrator answers 400 naming what the SKILL.md frontmatter is missing;
// `api` keeps that reason on the thrown message.
export const createSkill = createServerFn({ method: "POST" })
  .validator((input: { repository: string | null; content: string }) => input)
  .handler(
    async ({ data }): Promise<Skill> =>
      api<Skill>("/skills", {
        body: { content: data.content, repository: data.repository },
        method: "POST",
      })
  );

export const getTodos = createServerFn({ method: "GET" }).handler(
  async (): Promise<Todo[]> => api<Todo[]>("/todos")
);

export const getProjects = createServerFn({ method: "GET" }).handler(
  async (): Promise<Project[]> => api<Project[]>("/projects")
);

export const createProject = createServerFn({ method: "POST" })
  .validator((repository: string) => repository)
  .handler(
    async ({ data }): Promise<Project> =>
      api<Project>("/projects", {
        body: { repository: data },
        method: "POST",
      })
  );

// A prefix is unique across projects: the orchestrator answers 409 naming the
// project that already owns it, and `api` keeps that name on the message.
export const updateProjectPrefix = createServerFn({ method: "POST" })
  .validator((input: { id: string; prefix: string }) => input)
  .handler(
    async ({ data }): Promise<Project> =>
      api<Project>(`/projects/${data.id}`, {
        body: { prefix: data.prefix },
        method: "PATCH",
      })
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

export const createTask = createServerFn({ method: "POST" })
  .validator((spec: NewTaskSpec) => spec)
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>("/tasks", {
        body: {
          ...data,
          // `kind` is what the CLI sends for ad-hoc work, and `runner` is the
          // one optional field on TaskSpec without a serde default — it has to
          // travel even when nothing is pinned.
          kind: "run",
          runner: data.runner ?? null,
        },
        method: "POST",
      })
  );

// The orchestrator endpoint is still being built: expect 404 until it lands,
// and 503 with a human-readable reason when no runner or executor can serve
// it. `api` keeps that reason on the thrown message so the UI can show it.
export const enhancePrompt = createServerFn({ method: "POST" })
  .validator((input: { prompt: string; repository?: string }) => input)
  .handler(
    async ({ data }): Promise<{ prompt: string }> =>
      api<{ prompt: string }>("/enhance", { body: data, method: "POST" })
  );

export const getChats = createServerFn({ method: "GET" }).handler(
  async (): Promise<Chat[]> => api<Chat[]>("/chats")
);

export const getChat = createServerFn({ method: "GET" })
  .validator((id: string) => id)
  .handler(async ({ data }): Promise<Chat> => api<Chat>(`/chats/${data}`));

// The question is stored at once and answered in the background. The
// orchestrator answers 409 when --orchestrate is off or a question is already
// running; `api` keeps that reason on the thrown message.
export const createChat = createServerFn({ method: "POST" })
  .validator((input: { question: string }) => input)
  .handler(
    async ({ data }): Promise<Chat> =>
      api<Chat>("/chats", { body: data, method: "POST" })
  );

export const askChat = createServerFn({ method: "POST" })
  .validator((input: { id: string; question: string }) => input)
  .handler(
    async ({ data }): Promise<Chat> =>
      api<Chat>(`/chats/${data.id}/ask`, {
        body: { question: data.question },
        method: "POST",
      })
  );

export const updateTask = createServerFn({ method: "POST" })
  .validator(
    (input: { id: string; title?: string; archived?: boolean }) => input
  )
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>(`/tasks/${data.id}`, {
        body: { archived: data.archived, title: data.title },
        method: "PATCH",
      })
  );

export const updateChat = createServerFn({ method: "POST" })
  .validator(
    (input: { id: string; title?: string; archived?: boolean }) => input
  )
  .handler(
    async ({ data }): Promise<Chat> =>
      api<Chat>(`/chats/${data.id}`, {
        body: { archived: data.archived, title: data.title },
        method: "PATCH",
      })
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

export interface RetryTaskInput {
  executor?: Executor;
  id: string;
  runner?: string;
}

// Omitted overrides mean "same runner, same executor" to the orchestrator.
export const retryTask = createServerFn({ method: "POST" })
  .validator((input: RetryTaskInput) => input)
  .handler(
    async ({ data }): Promise<Task> =>
      api<Task>(`/tasks/${data.id}/retry`, {
        body: { executor: data.executor, runner: data.runner },
        method: "POST",
      })
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

// Omitting `files` keeps the ones the skill already carries.
export const updateSkill = createServerFn({ method: "POST" })
  .validator((input: { id: string; content: string }) => input)
  .handler(
    async ({ data }): Promise<Skill> =>
      api<Skill>(`/skills/${data.id}`, {
        body: { content: data.content },
        method: "PATCH",
      })
  );

export const deleteSkill = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<void> =>
      api<void>(`/skills/${data}`, { method: "DELETE" })
  );

export const approveSkill = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<Skill> =>
      api<Skill>(`/skills/${data}/approve`, { method: "POST" })
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

export const deleteTodo = createServerFn({ method: "POST" })
  .validator((id: string) => id)
  .handler(
    async ({ data }): Promise<void> =>
      api<void>(`/todos/${data}`, { method: "DELETE" })
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
  .validator(
    (input: { title: string; repository?: string; content: string }) => input
  )
  .handler(
    async ({ data }): Promise<Scratchpad> =>
      api<Scratchpad>("/scratchpads", { body: data, method: "POST" })
  );

export const updateScratchpad = createServerFn({ method: "POST" })
  .validator(
    (input: {
      id: string;
      title?: string;
      content?: string;
      archived?: boolean;
      /** Null moves the document back to every repository. */
      repository?: string | null;
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
          repository: data.repository,
          tags: data.tags,
          title: data.title,
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
