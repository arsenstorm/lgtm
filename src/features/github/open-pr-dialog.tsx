import { RiGitPullRequestLine } from "@remixicon/react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import type { AppError } from "@/lib/errors/app-error";

type OpenPrDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  prefillUrl: string;
  opening: boolean;
  /** Resolves to an error to show inline, or null on success. */
  onOpen: (url: string) => Promise<AppError | null>;
  onManageToken: () => void;
  /** Present only when the active repo has a GitHub remote; swaps to the
   * PR browser instead of pasting a URL. */
  onBrowsePrs?: () => void;
};

/**
 * Prompts for a pull-request URL and opens it into the review workspace. The
 * Rust command validates the URL and auth; its errors surface inline, and an
 * auth failure points at the token dialog.
 */
export function OpenPrDialog({
  open,
  onOpenChange,
  prefillUrl,
  opening,
  onOpen,
  onManageToken,
  onBrowsePrs,
}: OpenPrDialogProps) {
  const [url, setUrl] = useState(prefillUrl);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    if (open) {
      setUrl(prefillUrl);
      setError(null);
    }
  }, [open, prefillUrl]);

  const submit = async () => {
    if (!url.trim() || opening) {
      return;
    }
    setError(null);
    const result = await onOpen(url.trim());
    if (result) {
      setError(result);
      return;
    }
    onOpenChange(false);
  };

  const needsToken = error?.code === "authentication-failed";

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <RiGitPullRequestLine aria-hidden className="size-4" />
            Review a GitHub pull request
          </DialogTitle>
          <DialogDescription>
            Paste a pull-request URL to load its diff into the review workspace.
          </DialogDescription>
        </DialogHeader>

        <form
          className="flex flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <div className="flex items-center gap-2">
            <Input
              autoFocus
              onChange={(event) => setUrl(event.target.value)}
              placeholder="https://github.com/owner/repo/pull/123"
              value={url}
            />
            <Button
              className="shrink-0"
              disabled={opening || url.trim().length === 0}
              type="submit"
            >
              {opening ? <Spinner /> : null}
              Open
            </Button>
          </div>
        </form>

        {onBrowsePrs ? (
          <button
            className="w-fit text-muted-foreground text-sm underline-offset-4 hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onClick={onBrowsePrs}
            type="button"
          >
            or browse this repository&rsquo;s open pull requests
          </button>
        ) : null}

        {error ? (
          <div className="flex flex-col gap-2 rounded-2xl border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm">
            <p className="text-destructive">
              {needsToken
                ? "GitHub authentication failed. Connect a token with access to this repository, then try again."
                : error.message}
            </p>
            {needsToken ? (
              <Button
                className="w-fit"
                onClick={onManageToken}
                size="sm"
                variant="outline"
              >
                Manage GitHub token
              </Button>
            ) : null}
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
