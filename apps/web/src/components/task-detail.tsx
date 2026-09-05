import { FilePath } from "@/components/file-path";
import { LayersIcon } from "@/components/icons";
import { TaskComposer } from "@/components/task-composer";
import { TaskTranscript } from "@/components/task-transcript";
import type { Overlap, TaskDetail } from "@/lib/lgtm/types";

export function TaskDetailView({ detail }: { detail: TaskDetail }) {
  const { task, overlaps } = detail;

  return (
    <div className="mx-auto flex min-h-[calc(100dvh-4rem)] w-full max-w-7xl flex-col gap-6 px-4 pt-6 sm:px-6 lg:px-8">
      {overlaps.length > 0 ? (
        <div className="mx-auto flex w-full min-w-0 max-w-3xl flex-col gap-4">
          <OverlapPanel overlaps={overlaps} />
        </div>
      ) : null}

      <section
        aria-label="Task transcript"
        className="mx-auto flex w-full min-w-0 max-w-3xl flex-1 flex-col gap-6"
      >
        <TaskTranscript events={detail.events} task={task} />
        <TaskComposer task={task} />
      </section>
    </div>
  );
}

function OverlapPanel({ overlaps }: { overlaps: Overlap[] }) {
  return (
    <section className="rounded-lg border border-amber-500/35 bg-amber-500/5 p-4">
      <h2 className="flex items-center gap-2 font-medium text-amber-700 text-sm dark:text-amber-400">
        <LayersIcon className="size-4 shrink-0" />
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
