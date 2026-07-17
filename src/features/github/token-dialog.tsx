import {
  RiArrowDownSLine,
  RiCheckLine,
  RiExternalLinkLine,
  RiFileCopyLine,
  RiGithubLine,
  RiKey2Line,
} from "@remixicon/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "cnfast";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { Spinner } from "@/components/ui/spinner";
import { getSetting, setSetting } from "@/lib/db/settings";
import { toAppError } from "@/lib/errors/app-error";
import { setGithubToken } from "@/lib/tauri/github";
import { GITHUB_CLIENT_ID_KEY, useGithubAuth } from "./use-github-auth";

const VERIFICATION_HINT = "github.com/login/device";
const COPIED_RESET_MS = 1500;

type TokenDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

/**
 * Connect to GitHub. The primary path is OAuth device flow: one button starts
 * it, then a large pairing code plus a link to github.com/login/device. A
 * personal access token and a custom GitHub App client ID live under Advanced.
 * Closing the dialog mid-flow cancels it.
 */
export function TokenDialog({ open, onOpenChange }: TokenDialogProps) {
  const auth = useGithubAuth();
  const { status, refresh, cancel, clearError, needsClientId } = auth;
  const [advancedOpen, setAdvancedOpen] = useState(false);

  useEffect(() => {
    if (open) {
      clearError();
      refresh();
    }
  }, [open, refresh, clearError]);

  // A missing-client-ID failure steers the user into Advanced automatically.
  useEffect(() => {
    if (needsClientId) {
      setAdvancedOpen(true);
    }
  }, [needsClientId]);

  const inFlight = status === "starting" || status === "awaiting-approval";

  const handleOpenChange = useCallback(
    (next: boolean) => {
      if (!next && inFlight) {
        cancel();
      }
      onOpenChange(next);
    },
    [inFlight, cancel, onOpenChange]
  );

  return (
    <Dialog onOpenChange={handleOpenChange} open={open}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <RiGithubLine aria-hidden className="size-4" />
            Connect to GitHub
          </DialogTitle>
          <DialogDescription>
            Sign in to review pull requests, import comments, and submit
            reviews.
          </DialogDescription>
        </DialogHeader>

        <PrimarySection auth={auth} />

        <Separator />

        <AdvancedSection
          onOpenChange={setAdvancedOpen}
          onPatConnected={refresh}
          open={advancedOpen}
        />
      </DialogContent>
    </Dialog>
  );
}

function PrimarySection({ auth }: { auth: ReturnType<typeof useGithubAuth> }) {
  const { status, login, device, error, needsClientId, start, cancel } = auth;

  if (status === "connected" && login) {
    return <ConnectedPanel login={login} onDisconnect={auth.disconnect} />;
  }

  if (status === "starting" || status === "awaiting-approval") {
    return <PairingPanel device={device} onCancel={cancel} />;
  }

  return (
    <div className="flex flex-col gap-3">
      <Button onClick={start} size="lg">
        <RiGithubLine aria-hidden />
        Connect with GitHub
      </Button>
      {error ? (
        <Alert variant={needsClientId ? "default" : "destructive"}>
          <AlertTitle>
            {needsClientId ? "Choose how to connect" : "Couldn’t connect"}
          </AlertTitle>
          <AlertDescription>
            {needsClientId
              ? "No GitHub App is configured for device sign-in. Add your own client ID under Advanced, or paste a personal access token."
              : error.message}
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}

function ConnectedPanel({
  login,
  onDisconnect,
}: {
  login: string;
  onDisconnect: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);

  const disconnect = useCallback(async () => {
    setBusy(true);
    try {
      await onDisconnect();
      toast.success("Disconnected from GitHub");
    } catch (error) {
      toast.error("Could not disconnect", {
        description: toAppError(error).message,
      });
    } finally {
      setBusy(false);
    }
  }, [onDisconnect]);

  return (
    <div className="flex items-center justify-between rounded-2xl border bg-muted/40 px-4 py-3">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-muted-foreground text-xs">Connected as</span>
        <span className="truncate font-medium text-sm">{login}</span>
      </div>
      <Button disabled={busy} onClick={disconnect} size="sm" variant="outline">
        {busy ? <Spinner /> : null}
        Disconnect
      </Button>
    </div>
  );
}

function PairingPanel({
  device,
  onCancel,
}: {
  device: { userCode: string; verificationUri: string } | null;
  onCancel: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (copyTimer.current) {
        clearTimeout(copyTimer.current);
      }
    },
    []
  );

  const copy = useCallback(async () => {
    if (!device) {
      return;
    }
    try {
      await navigator.clipboard.writeText(device.userCode);
      setCopied(true);
      if (copyTimer.current) {
        clearTimeout(copyTimer.current);
      }
      copyTimer.current = setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch {
      toast.error("Could not copy the code");
    }
  }, [device]);

  return (
    <div className="flex flex-col items-center gap-4 rounded-2xl border bg-muted/30 px-4 py-6">
      <p className="text-center text-muted-foreground text-sm">
        Enter this code at {VERIFICATION_HINT}
      </p>

      {device ? (
        <div className="flex flex-col items-center gap-2">
          <output className="font-mono font-semibold text-4xl tabular-nums tracking-[0.35em]">
            {device.userCode}
          </output>
          <Button onClick={copy} size="sm" variant="ghost">
            {copied ? (
              <RiCheckLine aria-hidden />
            ) : (
              <RiFileCopyLine aria-hidden />
            )}
            {copied ? "Copied" : "Copy code"}
          </Button>
        </div>
      ) : (
        <div className="flex h-14 items-center">
          <Spinner className="size-6 text-muted-foreground" />
        </div>
      )}

      {device ? (
        <Button
          className="w-full"
          onClick={() => {
            openUrl(device.verificationUri).catch(() => {
              toast.error("Could not open the browser");
            });
          }}
          size="lg"
        >
          <RiExternalLinkLine aria-hidden />
          Open {VERIFICATION_HINT}
        </Button>
      ) : null}

      <div className="flex items-center gap-2 text-muted-foreground text-xs">
        <Spinner className="size-3.5" />
        Waiting for approval…
      </div>

      <Button onClick={onCancel} size="sm" variant="ghost">
        Cancel
      </Button>
    </div>
  );
}

function AdvancedSection({
  open,
  onOpenChange,
  onPatConnected,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPatConnected: () => void;
}) {
  return (
    <Collapsible onOpenChange={onOpenChange} open={open}>
      <CollapsibleTrigger className="flex w-full items-center justify-between rounded-lg py-1 text-left font-medium text-muted-foreground text-xs uppercase tracking-wide hover:text-foreground">
        Advanced
        <RiArrowDownSLine
          aria-hidden
          className={cn("size-4 transition-transform", open && "rotate-180")}
        />
      </CollapsibleTrigger>
      <CollapsibleContent className="flex flex-col gap-5 pt-4">
        <TokenField onConnected={onPatConnected} />
        <ClientIdField />
      </CollapsibleContent>
    </Collapsible>
  );
}

function TokenField({ onConnected }: { onConnected: () => void }) {
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);

  const connect = useCallback(async () => {
    if (!token.trim() || busy) {
      return;
    }
    setBusy(true);
    try {
      const login = await setGithubToken(token.trim());
      setToken("");
      onConnected();
      toast.success(`Connected as ${login}`);
    } catch (error) {
      // toAppError never carries the token; safe to surface.
      toast.error("Could not connect", {
        description: toAppError(error).message,
      });
    } finally {
      setBusy(false);
    }
  }, [token, busy, onConnected]);

  return (
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
        Personal access token
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
      <p className="text-muted-foreground text-xs">
        Fine-grained token with Pull requests read and write on the repositories
        you review.
      </p>
    </form>
  );
}

function ClientIdField() {
  const [clientId, setClientId] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const value = await getSetting(GITHUB_CLIENT_ID_KEY);
      if (!cancelled) {
        setClientId(value ?? "");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const save = useCallback(async () => {
    setBusy(true);
    try {
      await setSetting(GITHUB_CLIENT_ID_KEY, clientId.trim());
      toast.success("Client ID saved");
    } catch (error) {
      toast.error("Could not save client ID", {
        description: toAppError(error).message,
      });
    } finally {
      setBusy(false);
    }
  }, [clientId]);

  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        save();
      }}
    >
      <label
        className="font-medium text-muted-foreground text-xs"
        htmlFor="github-client-id"
      >
        GitHub App client ID
      </label>
      <div className="flex items-center gap-2">
        <Input
          autoComplete="off"
          id="github-client-id"
          onChange={(event) => setClientId(event.target.value)}
          placeholder="Iv1.…"
          value={clientId}
        />
        <Button className="shrink-0" disabled={busy} type="submit">
          {busy ? <Spinner /> : null}
          Save
        </Button>
      </div>
      <p className="text-muted-foreground text-xs">
        Optional. Use your own GitHub App registration for device-flow sign-in.
      </p>
    </form>
  );
}
