import {
  RiCloseLine,
  RiMore2Line,
  RiPencilLine,
  RiSparkling2Line,
} from "@remixicon/react";
import { useState } from "react";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { describeAnchorRange } from "@/features/reviews/anchor-range";
import { CommentComposer } from "@/features/reviews/comment-composer";
import { getExample } from "@/lib/db/memory-examples";
import type { MemoryExample, SuggestedComment } from "@/types/review";

const HIGH_MATCH_THRESHOLD = 0.85;
const POSSIBLE_MATCH_THRESHOLD = 0.72;

type ConfidenceLabel = { text: string; className: string };

const POSSIBLE_MATCH: ConfidenceLabel = {
  text: "Possible match",
  className:
    "border-violet-500/30 bg-violet-500/5 text-violet-600 dark:text-violet-400",
};

function confidenceLabel(adjustedConfidence: number): ConfidenceLabel {
  if (adjustedConfidence >= HIGH_MATCH_THRESHOLD) {
    return {
      text: "High match",
      className:
        "border-violet-500/40 bg-violet-500/10 text-violet-700 dark:text-violet-300",
    };
  }
  if (adjustedConfidence >= POSSIBLE_MATCH_THRESHOLD) {
    return POSSIBLE_MATCH;
  }
  // Below the engine floor should not render, but never mislabel as high.
  return POSSIBLE_MATCH;
}

export type SuggestionCardProps = {
  suggestion: SuggestedComment;
  onAccept: (suggestion: SuggestedComment) => void;
  onEditAndAccept: (suggestion: SuggestedComment, editedBody: string) => void;
  onDismiss: (suggestion: SuggestedComment) => void;
  onNeverAgain: (suggestion: SuggestedComment) => void;
  /** Loads the source example for provenance. Injectable for tests. */
  loadExample?: (id: string) => Promise<MemoryExample | null>;
};

/**
 * A ghost comment: a deterministic memory suggestion, never the user's own
 * draft. Dashed border, a sparkle icon, and a violet accent keep it visually
 * distinct from CommentCard. Nothing here publishes — the user must act.
 */
export function SuggestionCard({
  suggestion,
  onAccept,
  onEditAndAccept,
  onDismiss,
  onNeverAgain,
  loadExample = getExample,
}: SuggestionCardProps) {
  const [editing, setEditing] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const confidence = confidenceLabel(suggestion.adjustedConfidence);
  const caption = describeAnchorRange(
    suggestion.anchor.startLine,
    suggestion.anchor.endLine,
    suggestion.anchor.side
  );

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: pointer guard keeps the diff selection intact; no semantic role applies.
    <div
      className="flex flex-col gap-2 rounded-lg border border-violet-500/40 border-dashed bg-violet-500/5 p-2.5 text-card-foreground"
      onMouseDown={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="flex items-center gap-2">
        <RiSparkling2Line
          aria-hidden
          className="size-4 text-violet-600 dark:text-violet-400"
        />
        <span className="font-medium text-sm">Remembered suggestion</span>
        <Badge className={confidence.className} variant="outline">
          {confidence.text}
        </Badge>
        <span className="ml-auto font-medium font-mono text-muted-foreground text-xs">
          {caption}
        </span>
      </div>

      <p className="text-muted-foreground text-xs">{suggestion.explanation}</p>

      {editing ? (
        <CommentComposer
          caption="Edit before accepting"
          initialBody={suggestion.proposedBody}
          onCancel={() => setEditing(false)}
          onSubmit={(body) => {
            onEditAndAccept(suggestion, body);
            setEditing(false);
          }}
          submitLabel="Accept edit"
        />
      ) : (
        <>
          <p className="whitespace-pre-wrap break-words text-sm">
            {suggestion.proposedBody}
          </p>

          <Provenance
            loadExample={loadExample}
            memoryExampleId={suggestion.memoryExampleId}
          />

          <div className="flex items-center gap-1.5">
            <Button
              onClick={() => onAccept(suggestion)}
              size="xs"
              type="button"
            >
              Accept
            </Button>
            <Button
              onClick={() => setEditing(true)}
              size="xs"
              type="button"
              variant="ghost"
            >
              <RiPencilLine aria-hidden />
              Edit
            </Button>
            <Button
              onClick={() => onDismiss(suggestion)}
              size="xs"
              type="button"
              variant="ghost"
            >
              <RiCloseLine aria-hidden />
              Dismiss
            </Button>
            <div className="ml-auto">
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button
                      aria-label="More suggestion options"
                      size="icon-xs"
                      type="button"
                      variant="ghost"
                    >
                      <RiMore2Line aria-hidden />
                    </Button>
                  }
                />
                <DropdownMenuContent align="end">
                  <DropdownMenuItem
                    onClick={() => setConfirmOpen(true)}
                    variant="destructive"
                  >
                    Never suggest this again
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
        </>
      )}

      <NeverAgainDialog
        onConfirm={() => onNeverAgain(suggestion)}
        onOpenChange={setConfirmOpen}
        open={confirmOpen}
      />
    </div>
  );
}

function Provenance({
  memoryExampleId,
  loadExample,
}: {
  memoryExampleId: string;
  loadExample: (id: string) => Promise<MemoryExample | null>;
}) {
  const [example, setExample] = useState<MemoryExample | null>(null);
  const [loading, setLoading] = useState(false);

  const onOpen = (open: boolean) => {
    if (open && !(example || loading)) {
      setLoading(true);
      loadExample(memoryExampleId)
        .then(setExample)
        .finally(() => setLoading(false));
    }
  };

  return (
    <Popover onOpenChange={onOpen}>
      <PopoverTrigger
        render={
          <Button
            className="w-fit text-muted-foreground text-xs"
            size="xs"
            type="button"
            variant="ghost"
          >
            View original
          </Button>
        }
      />
      <PopoverContent align="start" className="w-80 text-sm">
        {loading ? (
          <p className="text-muted-foreground text-xs">Loading…</p>
        ) : null}
        {example ? (
          <div className="flex flex-col gap-2">
            <p className="font-mono text-muted-foreground text-xs">
              {example.filePath}
            </p>
            <pre className="max-h-40 overflow-auto rounded-md bg-muted p-2 font-mono text-xs">
              {example.selectedCode}
            </pre>
            <p className="whitespace-pre-wrap break-words text-sm">
              {example.commentBody}
            </p>
          </div>
        ) : null}
        {loading || example ? null : (
          <p className="text-muted-foreground text-xs">
            The original comment is no longer available.
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}

function NeverAgainDialog({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  return (
    <AlertDialog onOpenChange={onOpenChange} open={open}>
      <AlertDialogContent size="sm">
        <AlertDialogHeader>
          <AlertDialogTitle>Never suggest this again?</AlertDialogTitle>
          <AlertDialogDescription>
            This memory will never be suggested again. You can re-enable it
            later from your memory settings.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm} variant="destructive">
            Never suggest again
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
