import { RiGithubLine, RiKey2Line } from "@remixicon/react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
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
import { toAppError } from "@/lib/errors/app-error";
import {
  clearGithubToken,
  getGithubTokenStatus,
  setGithubToken,
} from "@/lib/tauri/github";

type TokenDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

type StatusState =
  | { kind: "loading" }
  | { kind: "connected"; login: string }
  | { kind: "disconnected" };

/**
 * Manage the GitHub personal access token: view status, set a new token
 * (validated by the Rust command, which returns the login), or remove it. The
 * token itself is only ever held in the password input and is cleared on
 * submit — never echoed back, stored in React state, or surfaced in toasts.
 */
export function TokenDialog({ open, onOpenChange }: TokenDialogProps) {
  const [status, setStatus] = useState<StatusState>({ kind: "loading" });
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);

  const refreshStatus = useCallback(async () => {
    setStatus({ kind: "loading" });
    try {
      const login = await getGithubTokenStatus();
      setStatus(
        login ? { kind: "connected", login } : { kind: "disconnected" }
      );
    } catch {
      setStatus({ kind: "disconnected" });
    }
  }, []);

  useEffect(() => {
    if (open) {
      setToken("");
      refreshStatus();
    }
  }, [open, refreshStatus]);

  const connect = useCallback(async () => {
    if (!token.trim() || busy) {
      return;
    }
    setBusy(true);
    try {
      const login = await setGithubToken(token.trim());
      setToken("");
      setStatus({ kind: "connected", login });
      toast.success(`Connected as ${login}`);
    } catch (error) {
      // toAppError never carries the token; safe to surface.
      toast.error("Could not connect", {
        description: toAppError(error).message,
      });
    } finally {
      setBusy(false);
    }
  }, [token, busy]);

  const remove = useCallback(async () => {
    setBusy(true);
    try {
      await clearGithubToken();
      setStatus({ kind: "disconnected" });
      toast.success("GitHub token removed");
    } catch (error) {
      toast.error("Could not remove token", {
        description: toAppError(error).message,
      });
    } finally {
      setBusy(false);
    }
  }, []);

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <RiGithubLine aria-hidden className="size-4" />
            GitHub access
          </DialogTitle>
          <DialogDescription>
            Connect a fine-grained personal access token with Pull requests read
            and write on the repositories you review.
          </DialogDescription>
        </DialogHeader>

        <StatusRow onRefresh={refreshStatus} status={status} />

        <form
          className="flex flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            connect();
          }}
        >
          <label
            className="font-medium text-muted-foreground text-xs"
            htmlFor="github-token"
          >
            {status.kind === "connected"
              ? "Replace token"
              : "Personal access token"}
          </label>
          <div className="flex items-center gap-2">
            <Input
              autoComplete="off"
              id="github-token"
              onChange={(event) => setToken(event.target.value)}
              placeholder="github_pat_…"
              type="password"
              value={token}
            />
            <Button
              className="shrink-0"
              disabled={busy || token.trim().length === 0}
              type="submit"
            >
              {busy ? <Spinner /> : <RiKey2Line aria-hidden />}
              Connect
            </Button>
          </div>
        </form>

        {status.kind === "connected" ? (
          <Button
            className="w-fit"
            disabled={busy}
            onClick={remove}
            size="sm"
            variant="outline"
          >
            Remove token
          </Button>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function StatusRow({
  status,
  onRefresh,
}: {
  status: StatusState;
  onRefresh: () => void;
}) {
  return (
    <div className="flex items-center justify-between rounded-2xl border bg-muted/40 px-3 py-2">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-muted-foreground text-xs">Status</span>
        <StatusValue status={status} />
      </div>
      {status.kind === "connected" ? (
        <Badge variant="secondary">Connected</Badge>
      ) : (
        <Button
          disabled={status.kind === "loading"}
          onClick={onRefresh}
          size="sm"
          variant="ghost"
        >
          Refresh
        </Button>
      )}
    </div>
  );
}

function StatusValue({ status }: { status: StatusState }) {
  if (status.kind === "loading") {
    return <span className="text-muted-foreground text-sm">Checking…</span>;
  }
  if (status.kind === "connected") {
    return <span className="truncate font-medium text-sm">{status.login}</span>;
  }
  return (
    <span className="text-muted-foreground text-sm">
      Not connected. If a token was set, it may be invalid or expired — replace
      it below.
    </span>
  );
}
