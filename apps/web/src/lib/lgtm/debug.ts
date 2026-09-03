/** Set to "1" while the account menu's "Stretch text" is on. Dev only: the
 *  server layer never reads it in a production build. */
export const DEBUG_COOKIE = "lgtm-debug";

// Values the client dereferences rather than displays: a status picks a glyph
// out of a map, an id becomes a url, a repository groups the sidebar. Stretch
// those and the page crashes instead of showing a layout bug.
const KEPT = new Set([
  "arch",
  "executor",
  "executors",
  "id",
  "model",
  "os",
  "priority",
  "reasoning_effort",
  "refs",
  "repository",
  "role",
  "running",
  "session",
  "source",
  "status",
  "task",
  "todo",
  "tool",
  "verification",
]);
const KEPT_SUFFIX = "_id";

const WORDS =
  "retry semantics identical regression endpoint swallowed orchestrator worktree runner checkpoint escalate diff review merged conflicted rollback transcript".split(
    " "
  );

/** A random tail: a sentence of random length, then one very long word, so a
 *  row has to both truncate and break. */
function tail(): string {
  const words = 6 + Math.floor(Math.random() * 40);
  const sentence = Array.from(
    { length: words },
    () => WORDS[Math.floor(Math.random() * WORDS.length)]
  ).join(" ");
  const word = Math.random()
    .toString(36)
    .slice(2)
    .repeat(4 + Math.floor(Math.random() * 6));
  return ` ${sentence} ${word}`;
}

/** Every string that is not a key, id or enum gets a random tail, all the
 *  way down. */
export function stretched<T>(value: T): T {
  return walk(value, null) as T;
}

function kept(key: string | null): boolean {
  return key === null || KEPT.has(key) || key.endsWith(KEPT_SUFFIX);
}

function walk(value: unknown, key: string | null): unknown {
  if (typeof value === "string") {
    return kept(key) ? value : `${value}${tail()}`;
  }
  if (Array.isArray(value)) {
    return value.map((item) => walk(item, key));
  }
  if (value !== null && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value)) {
      out[k] = walk(v, k);
    }
    return out;
  }
  return value;
}
