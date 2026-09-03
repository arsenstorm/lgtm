import { RunnerList } from "@/components/runner-list";
import { TaskList } from "@/components/task-list";
import { TodoRow } from "@/components/todo-row";
import type { RunnerStatus, Task, Todo } from "@/lib/lgtm/types";

export interface Referenced {
  runners: RunnerStatus[];
  tasks: Task[];
  todos: Todo[];
}

// Task and todo ids are eight hex characters; the agent is told to quote them
// exactly, so a bare id in the prose is the whole signal.
const SHORT_ID = /\b[0-9a-f]{8}\b/g;
// ponytail: runners are matched as single words, so a name with a space in it
// never gets a card; split on the name instead if one shows up.
const WORD_BREAK = /[^\w-]+/;

export function referencesIn(text: string, all: Referenced): Referenced {
  const ids = new Set(text.match(SHORT_ID) ?? []);
  const words = new Set(text.split(WORD_BREAK));
  return {
    runners: all.runners.filter((runner) => words.has(runner.info.name)),
    tasks: all.tasks.filter((task) => ids.has(task.id.slice(0, 8))),
    todos: all.todos.filter(
      (todo) => ids.has(todo.id.slice(0, 8)) || text.includes(todo.display_id)
    ),
  };
}

export function AnswerReferences({
  text,
  all,
}: {
  all: Referenced;
  text: string;
}) {
  const found = referencesIn(text, all);
  const empty =
    found.tasks.length === 0 &&
    found.todos.length === 0 &&
    found.runners.length === 0;
  if (empty) {
    return null;
  }
  return (
    <div className="flex flex-col gap-4">
      {found.tasks.length > 0 ? <TaskList tasks={found.tasks} /> : null}
      {found.todos.length > 0 ? (
        <ul className="-mx-2 divide-y divide-foreground/5" role="list">
          {found.todos.map((todo) => (
            <li key={todo.id}>
              <TodoRow className="pl-2" todo={todo} />
            </li>
          ))}
        </ul>
      ) : null}
      {found.runners.length > 0 ? <RunnerList runners={found.runners} /> : null}
    </div>
  );
}
