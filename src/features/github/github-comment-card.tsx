import {
  RiDeleteBinLine,
  RiExternalLinkLine,
  RiGithubLine,
} from "@remixicon/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "cnfast";
import { formatDistanceToNow } from "date-fns";
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
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { PrInlineComment } from "@/types/github";

function open(url: string) {
  openUrl(url).catch(() => {
    // Best-effort; a failed open shouldn't disrupt the review.
  });
}

type GithubCommentCardProps = {
  thread: PrInlineComment[];
  viewerLogin: string;
  onDelete: (commentId: number) => void;
  deleting?: boolean;
};

/**
 * An existing GitHub review-comment thread rendered inline in the diff (and in
 * the summary sheet for outdated ones). Deliberately distinct from draft
 * comments and memory suggestions: neutral border, a GitHub mark, one message
 * per author with replies indented. Bodies are plain text, never HTML. No reply
 * box in v1 — replying still happens on github.com.
 */
export function GithubCommentCard({
  thread,
  viewerLogin,
  onDelete,
  deleting,
}: GithubCommentCardProps) {
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    <div
      className="flex flex-col rounded-lg border bg-muted/30 text-card-foreground"
      onMouseDown={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="flex items-center gap-1.5 border-b px-2.5 py-1.5">
        <RiGithubLine aria-hidden className="size-3.5 text-muted-foreground" />
        <span className="font-medium text-muted-foreground text-xs">
          GitHub thread
        </span>
      </div>
      <div className="flex flex-col">
        {thread.map((comment, index) => (
          <Message
            comment={comment}
            deleting={deleting}
            isReply={index > 0}
            key={comment.id}
            onDelete={onDelete}
            own={comment.authorLogin === viewerLogin}
          />
        ))}
      </div>
    </div>
  );
}

function Message({
  comment,
  isReply,
  own,
  deleting,
  onDelete,
}: {
  comment: PrInlineComment;
  isReply: boolean;
  own: boolean;
  deleting?: boolean;
  onDelete: (commentId: number) => void;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-1 px-2.5 py-2",
        isReply && "ml-2.5 border-l pl-2.5"
      )}
    >
      <div className="flex items-center gap-2">
        <span className="font-medium text-xs">{comment.authorLogin}</span>
        <span className="text-muted-foreground text-xs">
          {formatDistanceToNow(new Date(comment.createdAt), {
            addSuffix: true,
          })}
        </span>
        <div className="ml-auto flex items-center gap-0.5">
          <IconAction
            label="Open on GitHub"
            onClick={() => open(comment.htmlUrl)}
          >
            <RiExternalLinkLine aria-hidden />
          </IconAction>
          {own ? (
            <DeleteAction
              deleting={deleting}
              onConfirm={() => onDelete(comment.id)}
            />
          ) : null}
        </div>
      </div>
      <p className="whitespace-pre-wrap break-words text-sm">{comment.body}</p>
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

function DeleteAction({
  deleting,
  onConfirm,
}: {
  deleting?: boolean;
  onConfirm: () => void;
}) {
  return (
    <AlertDialog>
      <AlertDialogTrigger
        render={
          <Button
            aria-label="Delete comment"
            disabled={deleting}
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
          <AlertDialogTitle>Delete this comment on GitHub?</AlertDialogTitle>
          <AlertDialogDescription>
            This deletes your comment on GitHub and can't be undone.
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
