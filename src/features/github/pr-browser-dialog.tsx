import {
  RiArrowLeftLine,
  RiGitMergeLine,
  RiGitPullRequestLine,
  RiRefreshLine,
} from "@remixicon/react";
import { cn } from "cnfast";
import { formatDistanceToNow } from "date-fns";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { type AppError, toAppError } from "@/lib/errors/app-error";
import { listPullRequests } from "@/lib/tauri/github";
import type { PullRequestSummary } from "@/types/github";

type PrBrowserDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  owner: string;
  repository: string;
  opening: boolean;
  /** Resolves to an error to show inline, or null on success. */
  onOpen: (url: string) => Promise<AppError | null>;
  onManageToken: () => void;
};

type ListState = {
  prs: PullRequestSummary[];
  loading: boolean;
  error: AppError | null;
};

type StateFilter = "open" | "closed" | "all";

// Height of the body region; matches ~7 PrRow-sized rows so every state
// (skeleton, error, empty, list) is the same size and the dialog never jumps.
const BODY_HEIGHT = "h-[400px]";

const EMPTY_LABELS: Record<StateFilter, string> = {
  open: "No open pull requests",
  closed: "No closed pull requests",
  all: "No pull requests",
};

function matchesFilter(pr: PullRequestSummary, filter: StateFilter): boolean {
  if (filter === "all") {
    return true;
  }
  // Merged PRs are "closed" on GitHub; they surface under the Closed filter.
  return pr.state === filter;
}

function usePrList(owner: string, repository: string, open: boolean) {
  const [state, setState] = useState<ListState>({
    prs: [],
    loading: true,
    error: null,
  });

  const load = useCallback(async () => {
    setState((prev) => ({ ...prev, loading: true, error: null }));
    try {
      const prs = await listPullRequests(owner, repository);
      setState({ prs, loading: false, error: null });
    } catch (error) {
      setState({ prs: [], loading: false, error: toAppError(error) });
    }
  }, [owner, repository]);

  // Fetch when the dialog opens, not on mount.
  useEffect(() => {
    if (open) {
      load();
    }
  }, [open, load]);

  return { ...state, reload: load };
}

/**
 * Browses a repository's pull requests (all states) and opens one into the
 * review workspace. Fetches the 100 most recently updated on open; selecting a
 * row hands off to the same open-PR flow used by the URL dialog. Closed and
 * merged PRs are selectable — opening them works downstream.
 */
export function PrBrowserDialog({
  open,
  onOpenChange,
  owner,
  repository,
  opening,
  onOpen,
  onManageToken,
}: PrBrowserDialogProps) {
  const { prs, loading, error, reload } = usePrList(owner, repository, open);
  const [openingUrl, setOpeningUrl] = useState<string | null>(null);
  const [openError, setOpenError] = useState<AppError | null>(null);
  const [filter, setFilter] = useState<StateFilter>("open");

  useEffect(() => {
    if (open) {
      setOpenError(null);
      setOpeningUrl(null);
      setFilter("open");
    }
  }, [open]);

  const counts = useMemo(
    () => ({
      open: prs.filter((pr) => pr.state === "open").length,
      closed: prs.filter((pr) => pr.state === "closed").length,
      all: prs.length,
    }),
    [prs]
  );
  const filtered = useMemo(
    () => prs.filter((pr) => matchesFilter(pr, filter)),
    [prs, filter]
  );

  const selectPr = useCallback(
    async (url: string) => {
      if (opening) {
        return;
      }
      setOpenError(null);
      setOpeningUrl(url);
      const result = await onOpen(url);
      if (result) {
        setOpenError(result);
        setOpeningUrl(null);
        return;
      }
      // Success flips the workspace to PR mode; just close.
      onOpenChange(false);
    },
    [opening, onOpen, onOpenChange]
  );

  const listError = openError ?? error;

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <RiGitPullRequestLine aria-hidden className="size-4" />
            Pull requests
          </DialogTitle>
          {/* Positioned to sit flush with the dialog's built-in close button. */}
          <Button
            aria-label="Refresh"
            className="absolute top-4 right-14 bg-secondary"
            disabled={loading}
            onClick={reload}
            size="icon-sm"
            variant="ghost"
          >
            {loading ? <Spinner /> : <RiRefreshLine aria-hidden />}
          </Button>
          <DialogDescription className="font-mono">
            {owner}/{repository}
          </DialogDescription>
        </DialogHeader>

        <StateFilterControl
          counts={counts}
          onChange={setFilter}
          showCounts={!(loading || listError)}
          value={filter}
        />

        <PrListBody
          error={listError}
          filter={filter}
          loading={loading}
          onManageToken={onManageToken}
          onSelect={selectPr}
          openingUrl={openingUrl}
          prs={filtered}
        />
      </DialogContent>
    </Dialog>
  );
}

const FILTER_OPTIONS: { value: StateFilter; label: string }[] = [
  { value: "open", label: "Open" },
  { value: "closed", label: "Closed" },
  { value: "all", label: "All" },
];

function StateFilterControl({
  value,
  counts,
  showCounts,
  onChange,
}: {
  value: StateFilter;
  counts: Record<StateFilter, number>;
  showCounts: boolean;
  onChange: (filter: StateFilter) => void;
}) {
  return (
    <div className="flex items-center gap-0.5 rounded-lg bg-muted p-0.5">
      {FILTER_OPTIONS.map((option) => {
        const active = value === option.value;
        return (
          <button
            aria-pressed={active}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1 font-medium text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              active
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground"
            )}
            key={option.value}
            onClick={() => onChange(option.value)}
            type="button"
          >
            {option.label}
            {showCounts ? (
              <span className="text-muted-foreground text-xs tabular-nums">
                {counts[option.value]}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

function PrListBody({
  loading,
  error,
  filter,
  prs,
  openingUrl,
  onSelect,
  onManageToken,
}: {
  loading: boolean;
  error: AppError | null;
  filter: StateFilter;
  prs: PullRequestSummary[];
  openingUrl: string | null;
  onSelect: (url: string) => void;
  onManageToken: () => void;
}) {
  if (loading) {
    return (
      <div className={cn(BODY_HEIGHT, "flex flex-col")}>
        {["a", "b", "c", "d", "e", "f", "g"].map((key) => (
          <PrRowSkeleton key={key} />
        ))}
      </div>
    );
  }

  if (error) {
    return (
      <div className={cn(BODY_HEIGHT, "flex flex-col justify-center")}>
        <PrListError error={error} onManageToken={onManageToken} />
      </div>
    );
  }

  if (prs.length === 0) {
    return (
      <div
        className={cn(
          BODY_HEIGHT,
          "flex items-center justify-center text-muted-foreground text-sm"
        )}
      >
        {EMPTY_LABELS[filter]}
      </div>
    );
  }

  return (
    <ScrollArea className={cn(BODY_HEIGHT, "-mx-2")}>
      <ul className="flex flex-col px-2">
        {prs.map((pr) => (
          <PrRow
            busy={openingUrl === pr.htmlUrl}
            disabled={openingUrl !== null}
            key={pr.number}
            onSelect={onSelect}
            pr={pr}
          />
        ))}
      </ul>
    </ScrollArea>
  );
}

// Mirrors PrRow's structure (px-2 py-2, gap-1, two text lines) so skeleton and
// loaded rows are pixel-identical in height — no layout shift on load.
function PrRowSkeleton() {
  return (
    <div className="flex flex-col gap-1 px-2 py-2">
      <div className="flex h-5 items-center">
        <Skeleton className="h-3.5 w-2/3" />
      </div>
      <div className="flex h-4 items-center">
        <Skeleton className="h-3 w-1/3" />
      </div>
    </div>
  );
}

function PrListError({
  error,
  onManageToken,
}: {
  error: AppError;
  onManageToken: () => void;
}) {
  const needsToken = error.code === "authentication-failed";
  const notAccessible = error.code === "repository-not-accessible";
  return (
    <div className="flex flex-col gap-2 rounded-2xl border border-destructive/40 bg-destructive/5 px-3 py-3 text-sm">
      <p className="text-destructive">
        {needsToken
          ? "Connect to GitHub to see this repository's pull requests."
          : error.message}
      </p>
      {notAccessible ? (
        <p className="text-muted-foreground text-xs">
          If this is a private repository, make sure the LGTM GitHub App is
          installed on it (GitHub → Settings → Applications → Installed GitHub
          Apps). Authorizing the app is not the same as installing it.
        </p>
      ) : null}
      {!needsToken && error.details ? (
        <p className="break-words text-muted-foreground text-xs">
          {error.details}
        </p>
      ) : null}
      {needsToken ? (
        <Button
          className="w-fit"
          onClick={onManageToken}
          size="sm"
          variant="outline"
        >
          Connect to GitHub
        </Button>
      ) : null}
    </div>
  );
}

function PrRow({
  pr,
  busy,
  disabled,
  onSelect,
}: {
  pr: PullRequestSummary;
  busy: boolean;
  disabled: boolean;
  onSelect: (url: string) => void;
}) {
  return (
    <li>
      <button
        className="flex w-full flex-col gap-1 rounded-lg px-2 py-2 text-left transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60"
        disabled={disabled}
        onClick={() => onSelect(pr.htmlUrl)}
        type="button"
      >
        <span className="flex min-w-0 items-baseline gap-2">
          <span className="shrink-0 font-mono text-muted-foreground text-xs tabular-nums">
            #{pr.number}
          </span>
          <span className="min-w-0 flex-1 truncate font-medium text-sm">
            {pr.title}
          </span>
          {pr.draft ? (
            <Badge className="shrink-0" variant="outline">
              Draft
            </Badge>
          ) : null}
          <PrStateBadge pr={pr} />
          {busy ? <Spinner className="size-3.5 shrink-0" /> : null}
        </span>
        <span className="flex min-w-0 items-center gap-2 text-muted-foreground text-xs">
          <span className="shrink-0">{pr.authorLogin}</span>
          <span className="flex min-w-0 items-center gap-1 font-mono">
            <span className="max-w-32 truncate">{pr.baseRef}</span>
            <RiArrowLeftLine aria-hidden className="size-3 shrink-0" />
            <span className="max-w-32 truncate">{pr.headRef}</span>
          </span>
          <span className="ml-auto shrink-0 whitespace-nowrap">
            {formatDistanceToNow(new Date(pr.updatedAt), { addSuffix: true })}
          </span>
        </span>
      </button>
    </li>
  );
}

function PrStateBadge({ pr }: { pr: PullRequestSummary }) {
  if (pr.merged) {
    return (
      <Badge
        className="shrink-0 gap-1 border-violet-600/40 text-violet-600 dark:text-violet-400"
        variant="outline"
      >
        <RiGitMergeLine aria-hidden />
        Merged
      </Badge>
    );
  }
  if (pr.state === "closed") {
    return (
      <Badge
        className="shrink-0 border-red-600/40 text-red-600 dark:text-red-400"
        variant="outline"
      >
        Closed
      </Badge>
    );
  }
  return null;
}
