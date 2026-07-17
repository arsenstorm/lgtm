import { RiGitMergeLine } from "@remixicon/react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { getSetting, setSetting } from "@/lib/db/settings";
import type { MergeMethod, PullRequestInfo } from "@/types/github";
import { ciTone, summarizeChecks } from "./ci-status";
import { mergeDisabledReason } from "./merge-logic";
import type { PrLiveState } from "./use-pr-state";

const METHOD_KEY = "merge-method";
const DELETE_BRANCH_KEY = "merge-delete-branch";
const DEFAULT_METHOD: MergeMethod = "squash";

const METHODS: { value: MergeMethod; label: string }[] = [
  { value: "squash", label: "Squash and merge" },
  { value: "merge", label: "Create a merge commit" },
  { value: "rebase", label: "Rebase and merge" },
];

function parseMethod(value: string | null): MergeMethod {
  return value === "merge" || value === "rebase" || value === "squash"
    ? value
    : DEFAULT_METHOD;
}

/**
 * Merge / close-reopen controls for a pull request. Below the submit block in
 * the review summary. Merged PRs collapse to a one-line summary; the real
 * enforcement of branch protections happens on GitHub, so failing checks only
 * demote the merge button to a "merge anyway" confirm rather than blocking it.
 */
export function DecisionSection({
  info,
  prLive,
}: {
  info: PullRequestInfo;
  prLive: PrLiveState;
}) {
  const state = info.state.toLowerCase();

  if (state === "merged") {
    return (
      <section className="flex flex-col gap-1 border-t p-4">
        <h3 className="flex items-center gap-1.5 font-medium text-sm">
          <RiGitMergeLine aria-hidden className="size-4" />
          Merged
        </h3>
        <p className="text-muted-foreground text-sm">
          Merged at{" "}
          <span className="font-mono">{info.headSha.slice(0, 7)}</span>.
        </p>
      </section>
    );
  }

  if (state === "closed") {
    return (
      <section className="flex flex-col gap-2 border-t p-4">
        <h3 className="font-medium text-sm">Closed</h3>
        <p className="text-muted-foreground text-sm">
          This pull request is closed.
        </p>
        <Button
          className="w-fit"
          disabled={prLive.busy.state}
          onClick={() => prLive.setState("open")}
          size="sm"
          variant="outline"
        >
          {prLive.busy.state ? <Spinner /> : null}
          Reopen
        </Button>
      </section>
    );
  }

  return <OpenDecision info={info} prLive={prLive} />;
}

function OpenDecision({
  info,
  prLive,
}: {
  info: PullRequestInfo;
  prLive: PrLiveState;
}) {
  const [method, setMethod] = useState<MergeMethod>(DEFAULT_METHOD);
  const [deleteBranch, setDeleteBranch] = useState(true);
  const [title, setTitle] = useState(`${info.title} (#${info.pullNumber})`);
  const [message, setMessage] = useState("");
  const [messageOpen, setMessageOpen] = useState(false);
  const [ackFailing, setAckFailing] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const inFlight = useRef(false);

  // Load persisted preferences once.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const [savedMethod, savedDelete] = await Promise.all([
        getSetting(METHOD_KEY),
        getSetting(DELETE_BRANCH_KEY),
      ]);
      if (cancelled) {
        return;
      }
      setMethod(parseMethod(savedMethod));
      if (savedDelete !== null) {
        setDeleteBranch(savedDelete === "true");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const { failing } = summarizeChecks(prLive.ciStatus?.checkRuns ?? []);
  const tone = ciTone(prLive.ciStatus);
  const gate = mergeDisabledReason({
    draft: info.draft,
    state: "open",
    mergeable: prLive.ciStatus?.mergeable ?? null,
    failingChecks: failing,
  });

  const usesTitle = method === "squash" || method === "merge";
  const needsAck = gate.warning !== null;
  const disabled =
    gate.blocked !== null ||
    prLive.busy.merge ||
    (needsAck && !ackFailing) ||
    (usesTitle && title.trim().length === 0);

  const changeMethod = (next: MergeMethod) => {
    setMethod(next);
    setSetting(METHOD_KEY, next).catch(() => {
      // Preference persistence is best-effort.
    });
  };

  const changeDeleteBranch = (next: boolean) => {
    setDeleteBranch(next);
    setSetting(DELETE_BRANCH_KEY, String(next)).catch(() => {
      // Preference persistence is best-effort.
    });
  };

  const doMerge = async () => {
    if (inFlight.current) {
      return;
    }
    inFlight.current = true;
    try {
      const error = await prLive.merge({
        expectedHeadSha: info.headSha,
        method,
        commitTitle: usesTitle ? title : null,
        commitMessage: usesTitle && message.trim() ? message : null,
        deleteBranch,
      });
      setConfirmOpen(false);
      if (error) {
        // GitHub's reason is authoritative (branch protections etc.).
        toast.error("Merge blocked", { description: error.message });
      }
    } finally {
      inFlight.current = false;
    }
  };

  return (
    <section className="flex flex-col gap-3 border-t p-4">
      <h3 className="font-medium text-sm">Merge</h3>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="merge-method">Method</Label>
        <NativeSelect
          className="w-full"
          id="merge-method"
          onChange={(event) => changeMethod(event.target.value as MergeMethod)}
          value={method}
        >
          {METHODS.map((option) => (
            <NativeSelectOption key={option.value} value={option.value}>
              {option.label}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      </div>

      {usesTitle ? (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="merge-title">Commit title</Label>
          <Input
            id="merge-title"
            onChange={(event) => setTitle(event.target.value)}
            value={title}
          />
        </div>
      ) : null}

      {usesTitle ? (
        <Collapsible onOpenChange={setMessageOpen} open={messageOpen}>
          <CollapsibleTrigger
            render={
              <Button
                className="w-fit px-0 text-muted-foreground text-xs"
                size="xs"
                type="button"
                variant="ghost"
              >
                {messageOpen ? "Hide commit message" : "Add commit message"}
              </Button>
            }
          />
          <CollapsibleContent>
            <Textarea
              aria-label="Commit message"
              className="mt-1.5"
              onChange={(event) => setMessage(event.target.value)}
              placeholder="Extended commit message (optional)…"
              value={message}
            />
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      <label
        className="flex cursor-pointer items-center gap-2 text-sm"
        htmlFor="merge-delete-branch"
      >
        <Checkbox
          checked={deleteBranch}
          id="merge-delete-branch"
          onCheckedChange={(next) => changeDeleteBranch(next === true)}
        />
        Delete branch after merge
      </label>

      {gate.blocked ? (
        <p className="text-muted-foreground text-xs">{gate.blocked}</p>
      ) : null}

      {needsAck ? (
        <label
          className="flex cursor-pointer items-start gap-2 text-amber-600 text-xs dark:text-amber-500"
          htmlFor="merge-ack-failing"
        >
          <Checkbox
            checked={ackFailing}
            id="merge-ack-failing"
            onCheckedChange={(next) => setAckFailing(next === true)}
          />
          {gate.warning}
        </label>
      ) : null}

      <Button
        className="w-fit"
        disabled={disabled}
        onClick={() => setConfirmOpen(true)}
      >
        <RiGitMergeLine aria-hidden />
        Merge pull request
      </Button>

      {tone === "unknown" ? (
        <p className="text-muted-foreground text-xs">
          Check status is unavailable — merging relies on GitHub's own
          protections.
        </p>
      ) : null}

      <div className="flex items-center gap-2 border-t pt-3">
        <CloseButton busy={prLive.busy.state} onClose={prLive.setState} />
      </div>

      <AlertDialog onOpenChange={setConfirmOpen} open={confirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Merge #{info.pullNumber} into {info.baseRef}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              {METHODS.find((m) => m.value === method)?.label}.{" "}
              {deleteBranch
                ? `The ${info.headRef} branch will be deleted.`
                : "The branch will be kept."}{" "}
              This posts to GitHub.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={prLive.busy.merge}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={prLive.busy.merge}
              onClick={(event) => {
                event.preventDefault();
                doMerge();
              }}
            >
              {prLive.busy.merge ? <Spinner /> : null}
              Merge
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}

function CloseButton({
  busy,
  onClose,
}: {
  busy: boolean;
  onClose: (next: "closed") => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button
        className="text-muted-foreground"
        disabled={busy}
        onClick={() => setOpen(true)}
        size="sm"
        variant="ghost"
      >
        {busy ? <Spinner /> : null}
        Close pull request
      </Button>
      <AlertDialog onOpenChange={setOpen} open={open}>
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>Close this pull request?</AlertDialogTitle>
            <AlertDialogDescription>
              It stays on GitHub and can be reopened later.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                event.preventDefault();
                setOpen(false);
                onClose("closed");
              }}
              variant="destructive"
            >
              Close pull request
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
