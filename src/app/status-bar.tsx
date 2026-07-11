import { RiChatThreadLine, RiSparkling2Line } from "@remixicon/react";
import { Button } from "@/components/ui/button";

type StatusBarProps = {
  changedCount: number;
  viewedCount: number;
  untrackedCount: number;
  comparisonLabel: string;
  headSha: string | null;
  commentCount: number;
  outdatedCount: number;
  suggestionCount: number;
  onOpenReview: () => void;
};

export function StatusBar({
  changedCount,
  viewedCount,
  untrackedCount,
  comparisonLabel,
  headSha,
  commentCount,
  outdatedCount,
  suggestionCount,
  onOpenReview,
}: StatusBarProps) {
  return (
    <footer className="flex h-6 shrink-0 items-center gap-3 border-t bg-background px-3 text-muted-foreground text-xs">
      <span>
        {changedCount} changed
        {changedCount > 0 ? ` · ${viewedCount}/${changedCount} viewed` : ""}
      </span>
      {untrackedCount > 0 ? <span>{untrackedCount} untracked</span> : null}
      {suggestionCount > 0 ? (
        <span className="flex items-center gap-1 text-violet-600 dark:text-violet-400">
          <RiSparkling2Line aria-hidden className="size-3" />
          {suggestionCount} suggestion{suggestionCount === 1 ? "" : "s"}
        </span>
      ) : null}
      <span className="mx-auto truncate">{comparisonLabel}</span>
      {headSha ? (
        <span className="font-mono">HEAD {headSha.slice(0, 7)}</span>
      ) : null}
      <Button
        className="h-5 gap-1 px-1.5 text-muted-foreground text-xs"
        onClick={onOpenReview}
        size="xs"
        type="button"
        variant="ghost"
      >
        <RiChatThreadLine aria-hidden className="size-3" />
        {commentCount} comment{commentCount === 1 ? "" : "s"}
        {outdatedCount > 0 ? (
          <span className="text-amber-600 dark:text-amber-400">
            · {outdatedCount} outdated
          </span>
        ) : null}
      </Button>
    </footer>
  );
}
