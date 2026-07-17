import {
  RiAlertLine,
  RiArrowRightLine,
  RiDeleteBinLine,
  RiPencilLine,
} from "@remixicon/react";
import { cn } from "cnfast";
import { formatDistanceToNow } from "date-fns";
import { useEffect, useRef, useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { ReviewComment } from "@/types/review";
import { describeAnchorRange } from "./anchor-range";
import { CommentComposer } from "./comment-composer";

type CommentCardProps = {
  comment: ReviewComment;
  isActive?: boolean;
  onEdit: (id: string, body: string) => void;
  onDelete: (id: string) => void;
  onNavigate?: (comment: ReviewComment) => void;
};

/**
 * A persisted draft comment: plain-text body (never rendered as HTML),
 * edit-in-place, delete-with-confirm, and an outdated warning when the diff
 * moved out from under it. Reused inline in the diff and in the summary sheet.
 */
export function CommentCard({
  comment,
  isActive,
  onEdit,
  onDelete,
  onNavigate,
}: CommentCardProps) {
  const [editing, setEditing] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const outdated = comment.status === "outdated";

  useEffect(() => {
    if (isActive) {
      ref.current?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }, [isActive]);

  const caption = describeAnchorRange(
    comment.anchor.startLine,
    comment.anchor.endLine,
    comment.anchor.side
  );

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    <div
      className={cn(
        "flex flex-col gap-2 rounded-lg border bg-card p-2.5 text-card-foreground shadow-sm transition-shadow",
        isActive && "ring-2 ring-ring",
        outdated && "border-amber-500/50"
      )}
      onMouseDown={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
      ref={ref}
    >
      <div className="flex items-center gap-2">
        <span className="font-medium font-mono text-muted-foreground text-xs">
          {caption}
        </span>
        {outdated ? (
          <Badge className="gap-1" variant="destructive">
            <RiAlertLine aria-hidden />
            Outdated
          </Badge>
        ) : null}
        <span className="ml-auto text-muted-foreground text-xs">
          {formatDistanceToNow(new Date(comment.updatedAt), {
            addSuffix: true,
          })}
        </span>
        {onNavigate ? (
          <IconAction label="Go to comment" onClick={() => onNavigate(comment)}>
            <RiArrowRightLine aria-hidden />
          </IconAction>
        ) : null}
        {editing ? null : (
          <IconAction label="Edit comment" onClick={() => setEditing(true)}>
            <RiPencilLine aria-hidden />
          </IconAction>
        )}
        <DeleteAction onConfirm={() => onDelete(comment.id)} />
      </div>

      {outdated ? (
        <p className="text-amber-600 text-xs dark:text-amber-400">
          The diff changed and this comment could no longer be placed reliably.
          Review it before exporting.
        </p>
      ) : null}

      {editing ? (
        <CommentComposer
          caption={caption}
          initialBody={comment.body}
          onCancel={() => setEditing(false)}
          onSubmit={(body) => {
            onEdit(comment.id, body);
            setEditing(false);
          }}
          submitLabel="Save"
        />
      ) : (
        <p className="whitespace-pre-wrap break-words text-sm">
          {comment.body}
        </p>
      )}
    </div>
  );
}

function IconAction({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            aria-label={label}
            onClick={onClick}
            size="icon-xs"
            type="button"
            variant="ghost"
          >
            {children}
          </Button>
        }
      />
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function DeleteAction({ onConfirm }: { onConfirm: () => void }) {
  return (
    <AlertDialog>
      <AlertDialogTrigger
        render={
          <Button
            aria-label="Delete comment"
            size="icon-xs"
            type="button"
            variant="ghost"
          >
            <RiDeleteBinLine aria-hidden />
          </Button>
        }
      />
      <AlertDialogContent size="sm">
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this comment?</AlertDialogTitle>
          <AlertDialogDescription>
            This removes the draft comment. It can't be undone.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm} variant="destructive">
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
