import { RiClipboardLine, RiFileList2Line } from "@remixicon/react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { splitPath } from "@/features/changes/file-change-meta";
import { ConversationSection } from "@/features/github/conversation-section";
import { DecisionSection } from "@/features/github/decision-section";
import { GithubCommentCard } from "@/features/github/github-comment-card";
import { ReviewsSection } from "@/features/github/reviews-section";
import {
  type SubmitContext,
  SubmitReview,
} from "@/features/github/submit-review";
import type { PrLiveState } from "@/features/github/use-pr-state";
import type { PrInlineComment, PullRequestInfo } from "@/types/github";
import type { ReviewComment } from "@/types/review";
import { CommentCard } from "./comment-card";
import { copyReviewMarkdown } from "./export-markdown";

type ReviewSummaryProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  byFile: Map<string, ReviewComment[]>;
  total: number;
  outdatedTotal: number;
  repoName: string;
  comparisonLabel: string;
  comments: ReviewComment[];
  onEdit: (id: string, body: string) => void;
  onDelete: (id: string) => void;
  onNavigate: (comment: ReviewComment) => void;
  /** Present in PR mode: enables grouped GitHub review submission. */
  submit?: SubmitContext;
  /** Present in PR mode: live GitHub state (CI, reviews, discussion, merge). */
  prLive?: PrLiveState;
  prInfo?: PullRequestInfo;
  /** GitHub inline threads that no longer place in the current diff. */
  outdatedThreads?: PrInlineComment[][];
};

/**
 * Right-side sheet with every draft comment grouped by file. Clicking a comment
 * navigates to it (and closes the sheet); edit/delete work in place; the header
 * exports the whole review as Markdown to the clipboard.
 */
export function ReviewSummary({
  open,
  onOpenChange,
  byFile,
  total,
  outdatedTotal,
  repoName,
  comparisonLabel,
  comments,
  onEdit,
  onDelete,
  onNavigate,
  submit,
  prLive,
  prInfo,
  outdatedThreads,
}: ReviewSummaryProps) {
  const prSections = prLive && prInfo ? { prLive, prInfo } : null;
  return (
    <Sheet onOpenChange={onOpenChange} open={open}>
      <SheetContent className="flex w-full flex-col gap-0 p-0 sm:max-w-md">
        <SheetHeader className="gap-2 border-b p-4">
          <SheetTitle>Review summary</SheetTitle>
          <SheetDescription>
            {total} comment{total === 1 ? "" : "s"}
            {outdatedTotal > 0 ? ` · ${outdatedTotal} outdated` : ""}
          </SheetDescription>
          <Button
            className="w-fit"
            disabled={total === 0}
            onClick={() =>
              copyReviewMarkdown({
                repoName,
                comparisonLabel,
                date: new Date(),
                comments,
              })
            }
            size="sm"
            type="button"
            variant="outline"
          >
            <RiClipboardLine aria-hidden />
            Copy as Markdown
          </Button>
        </SheetHeader>

        {total === 0 && !(submit || prSections) ? (
          <EmptyState />
        ) : (
          <ScrollArea className="min-h-0 flex-1">
            {total === 0 ? (
              <p className="p-4 text-muted-foreground text-sm">
                No comments yet. Select lines in the diff and press{" "}
                <span className="font-mono">c</span> to leave one, or submit a
                review with just a body below.
              </p>
            ) : (
              <div className="flex flex-col gap-4 p-4">
                {[...byFile.entries()].map(([path, fileComments]) => (
                  <FileGroup
                    comments={fileComments}
                    key={path}
                    onDelete={onDelete}
                    onEdit={onEdit}
                    onNavigate={onNavigate}
                    path={path}
                  />
                ))}
              </div>
            )}
            {submit && (!prInfo || prInfo.state.toLowerCase() === "open") ? (
              <SubmitReview comments={comments} submit={submit} />
            ) : null}
            {prSections ? (
              <PrSections
                outdatedThreads={outdatedThreads}
                prInfo={prSections.prInfo}
                prLive={prSections.prLive}
              />
            ) : null}
          </ScrollArea>
        )}
      </SheetContent>
    </Sheet>
  );
}

function FileGroup({
  path,
  comments,
  onEdit,
  onDelete,
  onNavigate,
}: {
  path: string;
  comments: ReviewComment[];
  onEdit: (id: string, body: string) => void;
  onDelete: (id: string) => void;
  onNavigate: (comment: ReviewComment) => void;
}) {
  const { dir, name } = splitPath(path);
  return (
    <section className="flex flex-col gap-2">
      <h3 className="flex items-baseline gap-1 truncate font-mono text-xs">
        {dir ? <span className="text-muted-foreground">{dir}</span> : null}
        <span className="font-medium">{name}</span>
        <span className="ml-1 text-muted-foreground">({comments.length})</span>
      </h3>
      <div className="flex flex-col gap-2">
        {comments.map((comment) => (
          <CommentCard
            comment={comment}
            key={comment.id}
            onDelete={onDelete}
            onEdit={onEdit}
            onNavigate={onNavigate}
          />
        ))}
      </div>
    </section>
  );
}

function PrSections({
  prLive,
  prInfo,
  outdatedThreads,
}: {
  prLive: PrLiveState;
  prInfo: PullRequestInfo;
  outdatedThreads?: PrInlineComment[][];
}) {
  return (
    <>
      <DecisionSection info={prInfo} prLive={prLive} />
      <ReviewsSection
        busy={prLive.busy.dismiss}
        onDismiss={prLive.dismiss}
        reviews={prLive.reviews}
      />
      {outdatedThreads && outdatedThreads.length > 0 ? (
        <OutdatedGithub
          busy={prLive.busy.deleteComment}
          onDelete={prLive.deleteInlineComment}
          threads={outdatedThreads}
          viewerLogin={prInfo.viewerLogin}
        />
      ) : null}
      <ConversationSection
        busy={prLive.busy.comment}
        comments={prLive.conversation}
        onAdd={prLive.addComment}
      />
    </>
  );
}

function OutdatedGithub({
  threads,
  viewerLogin,
  busy,
  onDelete,
}: {
  threads: PrInlineComment[][];
  viewerLogin: string;
  busy: boolean;
  onDelete: (commentId: number) => void;
}) {
  return (
    <section className="flex flex-col gap-2 border-t p-4">
      <h3 className="font-medium text-sm">
        Outdated on GitHub ({threads.length})
      </h3>
      <p className="text-muted-foreground text-xs">
        These GitHub comments no longer match a line in the current diff.
      </p>
      <div className="flex flex-col gap-2">
        {threads.map((thread) => (
          <GithubCommentCard
            deleting={busy}
            key={thread[0].id}
            onDelete={onDelete}
            thread={thread}
            viewerLogin={viewerLogin}
          />
        ))}
      </div>
    </section>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 p-8 text-center">
      <div className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground">
        <RiFileList2Line aria-hidden className="size-6" />
      </div>
      <p className="font-medium text-sm">No comments yet</p>
      <p className="text-muted-foreground text-sm">
        Select lines in the diff and press <span className="font-mono">c</span>{" "}
        to leave a comment.
      </p>
    </div>
  );
}
