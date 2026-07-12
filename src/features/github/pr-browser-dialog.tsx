import {
  RiArrowLeftLine,
  RiGitPullRequestLine,
  RiRefreshLine,
} from "@remixicon/react";
import { formatDistanceToNow } from "date-fns";
import { useCallback, useEffect, useState } from "react";
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
import { listOpenPullRequests } from "@/lib/tauri/github";
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

function usePrList(owner: string, repository: string, open: boolean) {
  const [state, setState] = useState<ListState>({
    prs: [],
    loading: true,
    error: null,
  });

  const load = useCallback(async () => {
    setState((prev) => ({ ...prev, loading: true, error: null }));
    try {
      const prs = await listOpenPullRequests(owner, repository);
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
 * Browses a repository's open pull requests and opens one into the review
 * workspace. Fetches the 50 most recently updated on open; selecting a row
 * hands off to the same open-PR flow used by the URL dialog.
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

  useEffect(() => {
    if (open) {
      setOpenError(null);
      setOpeningUrl(null);
    }
  }, [open]);

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
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <div className="flex items-center justify-between gap-2 pr-6">
            <DialogTitle className="flex items-center gap-2">
              <RiGitPullRequestLine aria-hidden className="size-4" />
              Open pull requests
            </DialogTitle>
            <Button
              aria-label="Refresh"
              disabled={loading}
              onClick={reload}
              size="icon-sm"
              variant="ghost"
            >
              {loading ? <Spinner /> : <RiRefreshLine aria-hidden />}
            </Button>
          </div>
          <DialogDescription className="font-mono">
            {owner}/{repository}
          </DialogDescription>
        </DialogHeader>

        <PrListBody
          error={listError}
          loading={loading}
          onManageToken={onManageToken}
          onSelect={selectPr}
          openingUrl={openingUrl}
          prs={prs}
        />
      </DialogContent>
    </Dialog>
  );
}

function PrListBody({
  loading,
  error,
  prs,
  openingUrl,
  onSelect,
  onManageToken,
}: {
  loading: boolean;
  error: AppError | null;
  prs: PullRequestSummary[];
  openingUrl: string | null;
  onSelect: (url: string) => void;
  onManageToken: () => void;
}) {
  if (loading) {
    return (
      <div className="flex flex-col gap-1">
        {["a", "b", "c", "d", "e"].map((key) => (
          <Skeleton className="h-12 rounded-lg" key={key} />
        ))}
      </div>
    );
  }

  if (error) {
    return <PrListError error={error} onManageToken={onManageToken} />;
  }

  if (prs.length === 0) {
    return (
      <p className="py-8 text-center text-muted-foreground text-sm">
        No open pull requests
      </p>
    );
  }

  return (
    <ScrollArea className="-mx-2 max-h-[60vh]">
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
