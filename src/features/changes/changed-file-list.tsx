import type { FileDiffMetadata } from "@pierre/diffs/react";
import {
  RiChat1Line,
  RiCheckLine,
  RiQuestionLine,
  RiSparkling2Line,
} from "@remixicon/react";
import { cn } from "cnfast";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { FileCommentCount } from "@/features/reviews/use-review-comments";
import {
  changeGlyph,
  fileStats,
  isDisplayable,
  splitPath,
  UNTRACKED_GLYPH,
} from "./file-change-meta";

type ChangedFileListProps = {
  files: FileDiffMetadata[];
  untracked: string[];
  selectedFile: string | null;
  viewed: Set<string>;
  loading: boolean;
  commentCounts: Map<string, FileCommentCount>;
  suggestionCounts: Map<string, number>;
  onSelect: (name: string) => void;
  onToggleViewed: (name: string) => void;
};

export function ChangedFileList({
  files,
  untracked,
  selectedFile,
  viewed,
  loading,
  commentCounts,
  suggestionCounts,
  onSelect,
  onToggleViewed,
}: ChangedFileListProps) {
  if (loading) {
    return (
      <div className="flex flex-col gap-1 p-2">
        {["a", "b", "c", "d", "e"].map((key) => (
          <Skeleton className="h-8 rounded-lg" key={key} />
        ))}
      </div>
    );
  }

  if (files.length === 0 && untracked.length === 0) {
    return (
      <p className="p-4 text-center text-muted-foreground text-sm">
        No files to show.
      </p>
    );
  }

  const untrackedSet = new Set(untracked);
  const shownNames = new Set(files.map((file) => file.name));
  const listOnly = untracked.filter((path) => !shownNames.has(path));

  return (
    <div className="flex flex-col py-1">
      {files.map((file) => (
        <FileRow
          comments={commentCounts.get(file.name)}
          file={file}
          isSelected={file.name === selectedFile}
          isUntracked={untrackedSet.has(file.name)}
          isViewed={viewed.has(file.name)}
          key={file.name}
          onSelect={onSelect}
          onToggleViewed={onToggleViewed}
          suggestions={suggestionCounts.get(file.name) ?? 0}
        />
      ))}

      {listOnly.length > 0 ? (
        <div className="mt-2">
          <div className="flex items-center gap-1.5 px-3 py-1.5 text-muted-foreground text-xs uppercase tracking-wide">
            Untracked
            <span className="lowercase tracking-normal">(not shown)</span>
          </div>
          <ul>
            {listOnly.map((path) => (
              <UntrackedRow key={path} path={path} />
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

function FileRow({
  file,
  isSelected,
  isUntracked,
  isViewed,
  comments,
  suggestions,
  onSelect,
  onToggleViewed,
}: {
  file: FileDiffMetadata;
  isSelected: boolean;
  isUntracked: boolean;
  isViewed: boolean;
  comments: FileCommentCount | undefined;
  suggestions: number;
  onSelect: (name: string) => void;
  onToggleViewed: (name: string) => void;
}) {
  const glyph = isUntracked ? UNTRACKED_GLYPH : changeGlyph(file.type);
  const { additions, deletions } = fileStats(file);
  const { dir, name } = splitPath(file.name);
  const displayable = isDisplayable(file);

  return (
    <div
      className={cn(
        "group flex items-center gap-2 px-2 pr-1",
        isSelected && "bg-muted"
      )}
    >
      <button
        aria-current={isSelected}
        className={cn(
          "flex min-w-0 flex-1 items-center gap-2 rounded-md py-1.5 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          isViewed && "opacity-55"
        )}
        onClick={() => onSelect(file.name)}
        type="button"
      >
        <span
          aria-label={glyph.label}
          className={cn(
            "w-3.5 shrink-0 text-center font-mono font-semibold text-xs",
            glyph.className
          )}
          role="img"
          title={glyph.label}
        >
          {glyph.letter}
        </span>
        <span className="flex min-w-0 flex-1 items-baseline truncate font-mono text-xs">
          {dir ? <span className="text-muted-foreground">{dir}</span> : null}
          <span className="text-foreground">{name}</span>
          {displayable ? null : (
            <RiQuestionLine
              aria-label="Cannot be displayed"
              className="ml-1 inline size-3 shrink-0 self-center text-muted-foreground"
            />
          )}
        </span>
        {suggestions > 0 ? (
          <span
            className="flex shrink-0 items-center gap-0.5 font-mono text-[11px] text-violet-600 tabular-nums dark:text-violet-400"
            title={`${suggestions} remembered suggestion${suggestions === 1 ? "" : "s"}`}
          >
            <RiSparkling2Line aria-hidden className="size-3" />
            {suggestions}
          </span>
        ) : null}
        {comments && comments.total > 0 ? (
          <span
            className={cn(
              "flex shrink-0 items-center gap-0.5 font-mono text-[11px] tabular-nums",
              comments.outdated > 0
                ? "text-amber-600 dark:text-amber-400"
                : "text-muted-foreground"
            )}
            title={
              comments.outdated > 0
                ? `${comments.total} comments · ${comments.outdated} outdated`
                : `${comments.total} comments`
            }
          >
            <RiChat1Line aria-hidden className="size-3" />
            {comments.total}
          </span>
        ) : null}
        <span className="shrink-0 font-mono text-[11px] tabular-nums">
          {additions > 0 ? (
            <span className="text-emerald-600 dark:text-emerald-400">
              +{additions}
            </span>
          ) : null}
          {additions > 0 && deletions > 0 ? " " : null}
          {deletions > 0 ? (
            <span className="text-red-600 dark:text-red-400">-{deletions}</span>
          ) : null}
        </span>
      </button>

      <ViewedToggle
        isViewed={isViewed}
        name={file.name}
        onToggle={onToggleViewed}
      />
    </div>
  );
}

function ViewedToggle({
  isViewed,
  name,
  onToggle,
}: {
  isViewed: boolean;
  name: string;
  onToggle: (name: string) => void;
}) {
  const label = isViewed ? "Mark as not viewed" : "Mark as viewed";
  return (
    <Tooltip>
      <TooltipTrigger
        aria-label={label}
        aria-pressed={isViewed}
        className={cn(
          "flex size-5 shrink-0 items-center justify-center rounded-md border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          isViewed
            ? "border-emerald-600/40 bg-emerald-600/15 text-emerald-600 dark:text-emerald-400"
            : "border-border text-transparent hover:border-foreground/40 hover:text-muted-foreground/60"
        )}
        onClick={() => onToggle(name)}
        render={<button type="button" />}
      >
        <RiCheckLine aria-hidden className="size-3.5" />
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function UntrackedRow({ path }: { path: string }) {
  const { dir, name } = splitPath(path);
  return (
    <li className="flex items-center gap-2 px-2 py-1.5">
      <span
        aria-label="Untracked"
        className="w-3.5 shrink-0 text-center font-mono font-semibold text-muted-foreground text-xs"
        role="img"
        title="Untracked"
      >
        U
      </span>
      <span className="flex min-w-0 flex-1 items-baseline truncate font-mono text-muted-foreground text-xs">
        {dir ? <span className="opacity-70">{dir}</span> : null}
        <span>{name}</span>
      </span>
    </li>
  );
}
