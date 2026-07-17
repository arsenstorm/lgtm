import {
  RiFolderOpenLine,
  RiGithubLine,
  RiGitPullRequestLine,
  RiGitRepositoryLine,
} from "@remixicon/react";
import { formatDistanceToNow } from "date-fns";
import { ErrorPanel } from "@/components/error-panel";
import { Button } from "@/components/ui/button";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { splitPath } from "@/features/changes/file-change-meta";
import type { AppError } from "@/lib/errors/app-error";
import { GITHUB_PATH_PREFIX } from "@/lib/github/repo-identity";
import type { RepositoryRecord } from "@/types/review";

type RepositoryPickerProps = {
  recents: RepositoryRecord[];
  recentsLoading: boolean;
  opening: boolean;
  error: AppError | null;
  onOpenPicker: () => void;
  onOpenPr: () => void;
  onManageToken: () => void;
  onOpenRecent: (record: RepositoryRecord) => void;
  onDismissError: () => void;
};

export function RepositoryPicker({
  recents,
  recentsLoading,
  opening,
  error,
  onOpenPicker,
  onOpenPr,
  onManageToken,
  onOpenRecent,
  onDismissError,
}: RepositoryPickerProps) {
  return (
    <main className="flex h-dvh w-full items-center justify-center overflow-y-auto bg-background p-6">
      <div className="flex w-full max-w-xl flex-col gap-6">
        <header className="flex flex-col items-center gap-3 text-center">
          <div className="flex size-12 items-center justify-center rounded-2xl bg-muted">
            <RiGitRepositoryLine aria-hidden className="size-6" />
          </div>
          <div className="flex flex-col gap-1">
            <h1 className="font-semibold text-xl tracking-tight">LGTM</h1>
            <p className="text-muted-foreground text-sm">
              Open a local repository or a GitHub pull request to start
              reviewing.
            </p>
          </div>
          <div className="flex flex-wrap items-center justify-center gap-2">
            <Button disabled={opening} onClick={onOpenPicker} size="lg">
              <RiFolderOpenLine aria-hidden />
              {opening ? "Opening…" : "Open repository…"}
            </Button>
            <Button
              disabled={opening}
              onClick={onOpenPr}
              size="lg"
              variant="outline"
            >
              <RiGitPullRequestLine aria-hidden />
              Review a GitHub pull request
            </Button>
          </div>
          <Button onClick={onManageToken} size="sm" variant="ghost">
            <RiGithubLine aria-hidden />
            Connect to GitHub
          </Button>
        </header>

        {error ? (
          <ErrorPanel
            error={error}
            onRetry={onDismissError}
            title="Could not open repository"
          />
        ) : null}

        <section className="flex flex-col gap-2">
          <h2 className="px-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">
            Recent repositories
          </h2>
          <RecentList
            loading={recentsLoading}
            onOpenRecent={onOpenRecent}
            opening={opening}
            recents={recents}
          />
        </section>
      </div>
    </main>
  );
}

function RecentList({
  recents,
  loading,
  opening,
  onOpenRecent,
}: {
  recents: RepositoryRecord[];
  loading: boolean;
  opening: boolean;
  onOpenRecent: (record: RepositoryRecord) => void;
}) {
  if (loading) {
    return (
      <div className="flex flex-col gap-1.5">
        {["a", "b", "c"].map((key) => (
          <Skeleton className="h-12 rounded-xl" key={key} />
        ))}
      </div>
    );
  }

  if (recents.length === 0) {
    return (
      <Empty className="py-10">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <RiGitRepositoryLine aria-hidden />
          </EmptyMedia>
          <EmptyTitle>No recent repositories</EmptyTitle>
          <EmptyDescription>
            Repositories you open will appear here for quick access.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <ScrollArea className="max-h-72">
      <ul className="flex flex-col gap-1.5">
        {recents.map((record) => {
          const isGithub = record.path.startsWith(GITHUB_PATH_PREFIX);
          return (
            <li key={record.id}>
              <button
                className="flex w-full items-center gap-3 rounded-xl border bg-card px-3 py-2.5 text-left transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
                disabled={opening}
                onClick={() => onOpenRecent(record)}
                type="button"
              >
                {isGithub ? (
                  <RiGithubLine
                    aria-hidden
                    className="size-4 shrink-0 text-muted-foreground"
                  />
                ) : (
                  <RiGitRepositoryLine
                    aria-hidden
                    className="size-4 shrink-0 text-muted-foreground"
                  />
                )}
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate font-medium text-sm">
                    {record.displayName}
                  </span>
                  <span className="truncate text-muted-foreground text-xs">
                    {isGithub
                      ? "GitHub pull request"
                      : splitPath(record.path).dir || record.path}
                  </span>
                </span>
                <span className="shrink-0 text-muted-foreground text-xs">
                  {formatDistanceToNow(new Date(record.lastOpenedAt), {
                    addSuffix: true,
                  })}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </ScrollArea>
  );
}
