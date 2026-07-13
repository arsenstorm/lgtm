import {
  type DiffLineAnnotation,
  FileDiff,
  type FileDiffMetadata,
  type SelectedLineRange,
  WorkerPoolContext,
} from "@pierre/diffs/react";
import { RiFileForbidLine, RiGitCommitLine } from "@remixicon/react";
import { memo, useCallback, useMemo } from "react";
import { ErrorPanel } from "@/components/error-panel";
import { Skeleton } from "@/components/ui/skeleton";
import { isDisplayable } from "@/features/changes/file-change-meta";
import { GithubCommentCard } from "@/features/github/github-comment-card";
import { SuggestionCard } from "@/features/memory/suggestion-card";
import { describeAnchorRange } from "@/features/reviews/anchor-range";
import { CommentCard } from "@/features/reviews/comment-card";
import { CommentComposer } from "@/features/reviews/comment-composer";
import { commentAnnotationSide } from "@/features/reviews/use-review-comments";
import { anchorSideToPatchSide } from "@/lib/diff/anchor";
import type { AppError } from "@/lib/errors/app-error";
import { placeInlineComment } from "@/lib/github/inline-comment-map";
import type { PrInlineComment } from "@/types/github";
import type { ReviewComment, SuggestedComment } from "@/types/review";
import { useCodeDragSelect } from "./use-code-drag-select";
import { diffWorkerPool } from "./worker-pool";

export type DiffView = "split" | "unified";

type AnnotationMeta =
  | { kind: "comment"; comment: ReviewComment }
  | { kind: "composer" }
  | { kind: "suggestion"; suggestion: SuggestedComment }
  | { kind: "github-comment"; thread: PrInlineComment[] };

export type DiffAnnotationProps = {
  comments: ReviewComment[];
  suggestions: SuggestedComment[];
  /** Existing GitHub inline threads placed in this file (top-level + replies). */
  githubThreads: PrInlineComment[][];
  githubViewerLogin: string;
  onDeleteGithubComment: (commentId: number) => void;
  selection: SelectedLineRange | null;
  composerOpen: boolean;
  activeCommentId: string | null;
  onSelectionChange: (range: SelectedLineRange | null) => void;
  onSaveComment: (body: string) => void;
  onCancelComposer: () => void;
  onEditComment: (id: string, body: string) => void;
  onDeleteComment: (id: string) => void;
  onAcceptSuggestion: (suggestion: SuggestedComment) => void;
  onEditAcceptSuggestion: (
    suggestion: SuggestedComment,
    editedBody: string
  ) => void;
  onDismissSuggestion: (suggestion: SuggestedComment) => void;
  onNeverAgainSuggestion: (suggestion: SuggestedComment) => void;
};

type DiffViewerProps = DiffAnnotationProps & {
  file: FileDiffMetadata | null;
  hasFiles: boolean;
  view: DiffView;
  theme: "light" | "dark";
  comparisonKey: string;
  comparisonLabel: string;
  loading: boolean;
  error: AppError | null;
  onRetry: () => void;
};

export function DiffViewer({
  file,
  hasFiles,
  view,
  theme,
  comparisonKey,
  comparisonLabel,
  loading,
  error,
  onRetry,
  ...annotationProps
}: DiffViewerProps) {
  const dragSelectHandlers = useCodeDragSelect(
    annotationProps.onSelectionChange
  );

  if (error) {
    return (
      <CenteredMessage>
        <ErrorPanel
          error={error}
          onRetry={onRetry}
          title="Could not load diff"
        />
      </CenteredMessage>
    );
  }

  if (loading) {
    return <DiffSkeleton />;
  }

  if (!hasFiles) {
    return (
      <EmptyMessage
        icon={<RiGitCommitLine aria-hidden className="size-6" />}
        title="No changes"
      >
        {comparisonLabel} has no changes to review.
      </EmptyMessage>
    );
  }

  if (!file) {
    return (
      <EmptyMessage
        icon={<RiGitCommitLine aria-hidden className="size-6" />}
        title="Select a file"
      >
        Choose a file from the list to view its diff.
      </EmptyMessage>
    );
  }

  if (!isDisplayable(file)) {
    return (
      <EmptyMessage
        icon={<RiFileForbidLine aria-hidden className="size-6" />}
        title="Cannot display this file"
      >
        This looks like a binary file or an unsupported change. Its contents
        can't be rendered as a text diff.
      </EmptyMessage>
    );
  }

  return (
    <div className="h-full overflow-auto" {...dragSelectHandlers}>
      <RenderedDiff
        file={file}
        // Stable key per file + comparison + view forces a clean remount
        // instead of reusing a stale FileDiff instance. Annotations are passed
        // as props so drafts survive re-renders within a mounted instance.
        key={`${file.name}::${comparisonKey}::${view}`}
        theme={theme}
        view={view}
        {...annotationProps}
      />
    </div>
  );
}

const RenderedDiff = memo(function RenderedDiff({
  file,
  view,
  theme,
  comments,
  suggestions,
  githubThreads,
  githubViewerLogin,
  onDeleteGithubComment,
  selection,
  composerOpen,
  activeCommentId,
  onSelectionChange,
  onSaveComment,
  onCancelComposer,
  onEditComment,
  onDeleteComment,
  onAcceptSuggestion,
  onEditAcceptSuggestion,
  onDismissSuggestion,
  onNeverAgainSuggestion,
}: DiffAnnotationProps & {
  file: FileDiffMetadata;
  view: DiffView;
  theme: "light" | "dark";
}) {
  const options = useMemo(
    () => ({
      diffStyle: view,
      themeType: theme,
      stickyHeader: true,
      overflow: "scroll" as const,
      enableLineSelection: true,
      onLineSelected: onSelectionChange,
      onLineSelectionChange: onSelectionChange,
    }),
    [view, theme, onSelectionChange]
  );

  const lineAnnotations = useMemo<DiffLineAnnotation<AnnotationMeta>[]>(() => {
    const result: DiffLineAnnotation<AnnotationMeta>[] = [];
    for (const comment of comments) {
      result.push({
        side: commentAnnotationSide(comment),
        lineNumber: comment.anchor.endLine,
        metadata: { kind: "comment", comment },
      });
    }
    for (const suggestion of suggestions) {
      result.push({
        side: anchorSideToPatchSide(suggestion.anchor.side),
        lineNumber: suggestion.anchor.endLine,
        metadata: { kind: "suggestion", suggestion },
      });
    }
    for (const thread of githubThreads) {
      const placement = placeInlineComment(thread[0]);
      if (placement) {
        result.push({
          side: placement.side,
          lineNumber: placement.lineNumber,
          metadata: { kind: "github-comment", thread },
        });
      }
    }
    if (composerOpen && selection) {
      result.push({
        side: selection.endSide ?? selection.side ?? "additions",
        lineNumber: selection.end,
        metadata: { kind: "composer" },
      });
    }
    return result;
  }, [comments, suggestions, githubThreads, composerOpen, selection]);

  const composerCaption = useMemo(() => {
    if (!selection) {
      return "";
    }
    const side = selection.side ?? "additions";
    return `Commenting on ${describeAnchorRange(
      selection.start,
      selection.end,
      side === "additions" ? "new" : "old"
    )}`;
  }, [selection]);

  const renderAnnotation = useCallback(
    (annotation: DiffLineAnnotation<AnnotationMeta>) => {
      const meta = annotation.metadata;
      if (meta.kind === "composer") {
        return (
          <div className="px-3 py-1.5">
            <CommentComposer
              caption={composerCaption}
              onCancel={onCancelComposer}
              onSubmit={onSaveComment}
            />
          </div>
        );
      }
      if (meta.kind === "suggestion") {
        return (
          <div className="px-3 py-1.5">
            <SuggestionCard
              onAccept={onAcceptSuggestion}
              onDismiss={onDismissSuggestion}
              onEditAndAccept={onEditAcceptSuggestion}
              onNeverAgain={onNeverAgainSuggestion}
              suggestion={meta.suggestion}
            />
          </div>
        );
      }
      if (meta.kind === "github-comment") {
        return (
          <div className="px-3 py-1.5">
            <GithubCommentCard
              onDelete={onDeleteGithubComment}
              thread={meta.thread}
              viewerLogin={githubViewerLogin}
            />
          </div>
        );
      }
      return (
        <div className="px-3 py-1.5">
          <CommentCard
            comment={meta.comment}
            isActive={meta.comment.id === activeCommentId}
            onDelete={onDeleteComment}
            onEdit={onEditComment}
          />
        </div>
      );
    },
    [
      activeCommentId,
      composerCaption,
      githubViewerLogin,
      onCancelComposer,
      onSaveComment,
      onEditComment,
      onDeleteComment,
      onDeleteGithubComment,
      onAcceptSuggestion,
      onDismissSuggestion,
      onEditAcceptSuggestion,
      onNeverAgainSuggestion,
    ]
  );

  // The worker pool highlights off the main thread and caches results across
  // the keyed remounts above, so holding j/k switches files without jank.
  return (
    <WorkerPoolContext.Provider value={diffWorkerPool}>
      <FileDiff
        fileDiff={file}
        lineAnnotations={lineAnnotations}
        options={options}
        renderAnnotation={renderAnnotation}
        selectedLines={selection}
      />
    </WorkerPoolContext.Provider>
  );
});

function DiffSkeleton() {
  return (
    <div className="flex flex-col gap-2 p-4">
      <Skeleton className="h-7 w-64 rounded-lg" />
      <div className="flex flex-col gap-1.5">
        {Array.from({ length: 12 }, (_, i) => `line-${i}`).map((key) => (
          <Skeleton className="h-4 rounded" key={key} />
        ))}
      </div>
    </div>
  );
}

function CenteredMessage({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full w-full items-center justify-center p-6">
      {children}
    </div>
  );
}

function EmptyMessage({
  icon,
  title,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <CenteredMessage>
      <div className="flex max-w-sm flex-col items-center gap-2 text-center">
        <div className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground">
          {icon}
        </div>
        <h2 className="font-medium text-sm">{title}</h2>
        <p className="text-muted-foreground text-sm">{children}</p>
      </div>
    </CenteredMessage>
  );
}
