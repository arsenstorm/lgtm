import { RiErrorWarningLine, RiFileCopyLine } from "@remixicon/react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { AppError, AppErrorCode } from "@/lib/errors/app-error";

const FRIENDLY_MESSAGES: Partial<Record<AppErrorCode, string>> = {
  "git-unavailable":
    "Git isn't available on your system. Install Git and make sure it's on your PATH, then try again.",
  "not-a-git-repository":
    "This folder isn't a Git repository. Pick a folder that contains a .git directory.",
  "repository-not-found":
    "This repository could not be found. It may have been moved or deleted.",
};

type ErrorPanelProps = {
  error: AppError;
  title?: string;
  onRetry?: () => void;
};

export function ErrorPanel({ error, title, onRetry }: ErrorPanelProps) {
  const message = FRIENDLY_MESSAGES[error.code] ?? error.message;

  const copyDetails = async () => {
    const payload = error.details
      ? `${error.code}: ${error.message}\n\n${error.details}`
      : `${error.code}: ${error.message}`;
    try {
      await navigator.clipboard.writeText(payload);
      toast.success("Error details copied");
    } catch {
      toast.error("Could not copy to clipboard");
    }
  };

  return (
    <div className="mx-auto flex w-full max-w-lg flex-col gap-3 rounded-2xl border bg-card p-5 text-card-foreground">
      <div className="flex items-start gap-2.5">
        <RiErrorWarningLine
          aria-hidden
          className="mt-0.5 size-4 shrink-0 text-destructive"
        />
        <div className="flex flex-1 flex-col gap-1">
          <div className="flex items-center gap-2">
            <h2 className="font-medium text-sm">
              {title ?? "Something went wrong"}
            </h2>
            <Badge variant="outline">{error.code}</Badge>
          </div>
          <p className="text-muted-foreground text-sm">{message}</p>
        </div>
      </div>

      {error.details ? (
        <details className="rounded-xl border bg-muted/40 text-xs">
          <summary className="cursor-pointer select-none px-3 py-2 font-medium text-muted-foreground">
            Technical details
          </summary>
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all border-t px-3 py-2 font-mono text-muted-foreground">
            {error.details}
          </pre>
        </details>
      ) : null}

      <div className="flex items-center justify-end gap-2">
        <Button onClick={copyDetails} size="sm" variant="ghost">
          <RiFileCopyLine aria-hidden />
          Copy details
        </Button>
        {onRetry ? (
          <Button onClick={onRetry} size="sm" variant="outline">
            Try again
          </Button>
        ) : null}
      </div>
    </div>
  );
}
