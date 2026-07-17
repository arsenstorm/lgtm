import { RiErrorWarningLine, RiGithubLine } from "@remixicon/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useMemo, useRef, useState } from "react";
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
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { toAppError } from "@/lib/errors/app-error";
import { anchorToGithubComment } from "@/lib/github/anchor-map";
import { submitGithubReview } from "@/lib/tauri/github";
import type { GithubReviewEvent } from "@/types/github";
import type { ReviewComment } from "@/types/review";
import { canSubmit } from "./submit-review-logic";

export type SubmitContext = {
  owner: string;
  repository: string;
  pullNumber: number;
  /** Head SHA of the currently loaded bundle; re-verified by the Rust command. */
  headSha: string;
  onPublished: (ids: string[]) => void;
  onRevisionChanged: () => void;
};

const EVENTS: { value: GithubReviewEvent; label: string; hint: string }[] = [
  {
    value: "COMMENT",
    label: "Comment",
    hint: "Leave feedback without approval",
  },
  { value: "APPROVE", label: "Approve", hint: "Approve these changes" },
  {
    value: "REQUEST_CHANGES",
    label: "Request changes",
    hint: "Block until concerns are addressed",
  },
];

/**
 * PR review submission: pick an event + optional body and send every draft
 * comment as one grouped review. Outdated comments are excluded (GitHub can't
 * place them on the current head). A confirm step guards against accidental
 * submits; a ref guard guarantees exactly one request per confirmation.
 */
export function SubmitReview({
  comments,
  submit,
}: {
  comments: ReviewComment[];
  submit: SubmitContext;
}) {
  const [event, setEvent] = useState<GithubReviewEvent>("COMMENT");
  const [body, setBody] = useState("");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const inFlight = useRef(false);

  const drafts = useMemo(
    () => comments.filter((c) => c.status === "draft"),
    [comments]
  );
  const outdatedCount = useMemo(
    () => comments.filter((c) => c.status === "outdated").length,
    [comments]
  );

  const ready = canSubmit(event, body, drafts.length);
  const target = `${submit.owner}/${submit.repository} #${submit.pullNumber}`;

  const doSubmit = async () => {
    if (inFlight.current) {
      return;
    }
    inFlight.current = true;
    setSubmitting(true);
    try {
      const result = await submitGithubReview({
        owner: submit.owner,
        repository: submit.repository,
        pullNumber: submit.pullNumber,
        expectedHeadSha: submit.headSha,
        event,
        body,
        comments: drafts.map((c) => anchorToGithubComment(c.anchor, c.body)),
      });
      submit.onPublished(drafts.map((c) => c.id));
      setConfirmOpen(false);
      setBody("");
      toast.success("Review submitted", {
        action: {
          label: "View on GitHub",
          onClick: () => {
            openUrl(result.htmlUrl).catch(() => {
              toast.error("Could not open the browser");
            });
          },
        },
      });
    } catch (error) {
      const appError = toAppError(error);
      setConfirmOpen(false);
      if (appError.code === "pull-request-revision-changed") {
        toast.error("The pull request changed", {
          description:
            "Its head moved since you loaded it. Refreshing so your comments re-anchor — review, then submit again.",
        });
        submit.onRevisionChanged();
      } else {
        toast.error("Could not submit review", {
          description: appError.message,
        });
      }
    } finally {
      inFlight.current = false;
      setSubmitting(false);
    }
  };

  return (
    <section className="flex flex-col gap-3 border-t p-4">
      <h3 className="font-medium text-sm">Submit review</h3>

      <RadioGroup
        onValueChange={(value) => setEvent(value as GithubReviewEvent)}
        value={event}
      >
        {EVENTS.map((option) => (
          <label
            className="flex cursor-pointer items-start gap-2.5"
            htmlFor={`review-event-${option.value}`}
            key={option.value}
          >
            <RadioGroupItem
              className="mt-0.5"
              id={`review-event-${option.value}`}
              value={option.value}
            />
            <span className="flex flex-col">
              <span className="text-sm">{option.label}</span>
              <span className="text-muted-foreground text-xs">
                {option.hint}
              </span>
            </span>
          </label>
        ))}
      </RadioGroup>

      <Textarea
        aria-label="Review body"
        onChange={(e) => setBody(e.target.value)}
        placeholder="Leave an overall comment (optional)…"
        value={body}
      />

      <p className="text-muted-foreground text-sm">
        {drafts.length === 0
          ? "No draft comments — only the body above will be sent."
          : `${drafts.length} comment${drafts.length === 1 ? "" : "s"} will be sent as one review.`}
      </p>

      {outdatedCount > 0 ? (
        <p className="flex items-center gap-1.5 text-amber-600 text-xs dark:text-amber-500">
          <RiErrorWarningLine aria-hidden className="size-3.5 shrink-0" />
          {outdatedCount} outdated comment{outdatedCount === 1 ? "" : "s"} will
          not be submitted.
        </p>
      ) : null}

      {event === "COMMENT" && !ready ? (
        <p className="text-muted-foreground text-xs">
          Add a comment or a body — GitHub rejects an empty review.
        </p>
      ) : null}

      <Button
        className="w-fit"
        disabled={!ready || submitting}
        onClick={() => setConfirmOpen(true)}
      >
        <RiGithubLine aria-hidden />
        Submit to {target}
      </Button>

      <AlertDialog onOpenChange={setConfirmOpen} open={confirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Submit review to {target}?</AlertDialogTitle>
            <AlertDialogDescription>
              {EVENTS.find((e) => e.value === event)?.label} with{" "}
              {drafts.length} comment{drafts.length === 1 ? "" : "s"}
              {body.trim() ? " and a body" : ""}. This posts to GitHub and
              cannot be undone here.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={submitting}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              disabled={submitting}
              onClick={(event_) => {
                event_.preventDefault();
                doSubmit();
              }}
            >
              {submitting ? <Spinner /> : null}
              Submit review
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
