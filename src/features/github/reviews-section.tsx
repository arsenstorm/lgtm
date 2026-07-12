import { formatDistanceToNow } from "date-fns";
import { useMemo, useState } from "react";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import type { ReviewInfo } from "@/types/github";

type StateStyle = { label: string; className: string };

const STATE_STYLES: Record<string, StateStyle> = {
  APPROVED: {
    label: "Approved",
    className:
      "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  },
  CHANGES_REQUESTED: {
    label: "Changes requested",
    className:
      "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  COMMENTED: { label: "Commented", className: "text-muted-foreground" },
  DISMISSED: {
    label: "Dismissed",
    className: "text-muted-foreground opacity-70",
  },
};

function styleFor(state: string): StateStyle {
  return (
    STATE_STYLES[state] ?? { label: state, className: "text-muted-foreground" }
  );
}

const DISMISSABLE = new Set(["APPROVED", "CHANGES_REQUESTED"]);

/**
 * Existing reviews on the pull request: author, verdict badge, relative time,
 * and body (plain text, clamped). Approvals and change-requests can be
 * dismissed with a required message.
 */
export function ReviewsSection({
  reviews,
  busy,
  onDismiss,
}: {
  reviews: ReviewInfo[];
  busy: boolean;
  onDismiss: (reviewId: number, message: string) => Promise<boolean>;
}) {
  // PENDING is the viewer's own not-yet-submitted review; never show it here.
  const visible = useMemo(
    () => reviews.filter((review) => review.state !== "PENDING"),
    [reviews]
  );

  return (
    <section className="flex flex-col gap-2 border-t p-4">
      <h3 className="font-medium text-sm">Reviews</h3>
      {visible.length === 0 ? (
        <p className="text-muted-foreground text-sm">No reviews yet</p>
      ) : (
        <div className="flex flex-col gap-2">
          {visible.map((review) => (
            <ReviewRow
              busy={busy}
              key={review.id}
              onDismiss={onDismiss}
              review={review}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function ReviewRow({
  review,
  busy,
  onDismiss,
}: {
  review: ReviewInfo;
  busy: boolean;
  onDismiss: (reviewId: number, message: string) => Promise<boolean>;
}) {
  const [expanded, setExpanded] = useState(false);
  const style = styleFor(review.state);
  const body = review.body.trim();
  const clampable = body.length > 240;

  return (
    <div className="flex flex-col gap-1.5 rounded-lg border bg-card p-2.5">
      <div className="flex items-center gap-2">
        <span className="font-medium text-sm">{review.authorLogin}</span>
        <Badge className={style.className} variant="outline">
          {style.label}
        </Badge>
        {review.submittedAt ? (
          <span className="ml-auto text-muted-foreground text-xs">
            {formatDistanceToNow(new Date(review.submittedAt), {
              addSuffix: true,
            })}
          </span>
        ) : null}
      </div>

      {body ? (
        <p
          className={
            expanded
              ? "whitespace-pre-wrap break-words text-sm"
              : "line-clamp-3 whitespace-pre-wrap break-words text-sm"
          }
        >
          {body}
        </p>
      ) : null}

      <div className="flex items-center gap-2">
        {clampable ? (
          <Button
            className="w-fit px-0 text-muted-foreground text-xs"
            onClick={() => setExpanded((prev) => !prev)}
            size="xs"
            type="button"
            variant="ghost"
          >
            {expanded ? "Show less" : "Show more"}
          </Button>
        ) : null}
        {DISMISSABLE.has(review.state) ? (
          <DismissDialog
            busy={busy}
            onDismiss={(message) => onDismiss(review.id, message)}
          />
        ) : null}
      </div>
    </div>
  );
}

function DismissDialog({
  busy,
  onDismiss,
}: {
  busy: boolean;
  onDismiss: (message: string) => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState("");
  const trimmed = message.trim();

  const submit = async () => {
    if (!trimmed || busy) {
      return;
    }
    const ok = await onDismiss(message);
    if (ok) {
      setOpen(false);
      setMessage("");
    }
  };

  return (
    <>
      <Button
        className="ml-auto text-muted-foreground"
        onClick={() => setOpen(true)}
        size="xs"
        type="button"
        variant="ghost"
      >
        Dismiss
      </Button>
      <AlertDialog onOpenChange={setOpen} open={open}>
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>Dismiss this review?</AlertDialogTitle>
          </AlertDialogHeader>
          <Textarea
            aria-label="Dismissal message"
            onChange={(event) => setMessage(event.target.value)}
            placeholder="Why are you dismissing this review? (required)"
            value={message}
          />
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy}>Cancel</AlertDialogCancel>
            <Button
              disabled={!trimmed || busy}
              onClick={submit}
              type="button"
              variant="destructive"
            >
              {busy ? <Spinner /> : null}
              Dismiss
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
