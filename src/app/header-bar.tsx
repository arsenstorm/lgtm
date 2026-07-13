import {
  RiArrowDownSLine,
  RiArrowLeftLine,
  RiArrowLeftSLine,
  RiCheckLine,
  RiCloseLine,
  RiDownloadLine,
  RiErrorWarningLine,
  RiExternalLinkLine,
  RiGitBranchLine,
  RiGitMergeLine,
  RiGitPullRequestLine,
  RiLayoutColumnLine,
  RiLayoutRowLine,
  RiSettings4Line,
  RiSubtractLine,
} from "@remixicon/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "cnfast";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { ComparisonMode } from "@/features/changes/comparison";
import type { DiffView } from "@/features/diff/diff-viewer";
import { ciTone, summarizeChecks } from "@/features/github/ci-status";
import { parseGithubRemote } from "@/lib/github/remote";
import type { RepositoryInfo } from "@/types/git";
import type { CheckRunInfo, PrCiStatus, PullRequestInfo } from "@/types/github";

const WORKING_TREE_VALUE = "\u0000working-tree";

type HeaderBarProps = {
  info: RepositoryInfo;
  mode: ComparisonMode;
  view: DiffView;
  onModeChange: (mode: ComparisonMode) => void;
  onViewChange: (view: DiffView) => void;
  onClose: () => void;
  onOpenSettings: () => void;
  /** Present only when the repo has a GitHub remote; shows the PR browser. */
  onBrowsePrs?: () => void;
};

export function HeaderBar({
  info,
  mode,
  view,
  onModeChange,
  onViewChange,
  onClose,
  onOpenSettings,
  onBrowsePrs,
}: HeaderBarProps) {
  const remote = parseGithubRemote(info.remoteUrl);

  return (
    <header className="flex h-11 shrink-0 items-center gap-2 border-b bg-background px-2">
      <IconButton label="Close repository" onClick={onClose}>
        <RiArrowLeftSLine aria-hidden />
      </IconButton>

      <div className="flex min-w-0 items-center gap-2">
        {remote ? (
          <span className="truncate font-medium text-sm">
            <span className="text-muted-foreground">{remote.owner}/</span>
            {remote.repository}
          </span>
        ) : (
          <span className="truncate font-medium text-sm">
            {info.displayName}
          </span>
        )}
        {info.branches.length > 0 || info.remoteBranches.length > 0 ? (
          <HeadSelector info={info} mode={mode} onModeChange={onModeChange} />
        ) : (
          <BranchBadge info={info} />
        )}
        <span className="shrink-0 text-muted-foreground text-xs">vs</span>
        <ComparisonSelector
          info={info}
          mode={mode}
          onModeChange={onModeChange}
        />
      </div>

      <div className="flex-1" />

      <ViewToggle onViewChange={onViewChange} view={view} />

      {onBrowsePrs ? (
        <IconButton label="Pull requests" onClick={onBrowsePrs}>
          <RiGitPullRequestLine aria-hidden />
        </IconButton>
      ) : null}

      <IconButton label="Settings" onClick={onOpenSettings}>
        <RiSettings4Line aria-hidden />
      </IconButton>
    </header>
  );
}

type PrHeaderBarProps = {
  info: PullRequestInfo;
  view: DiffView;
  ciStatus: PrCiStatus | null;
  onViewChange: (view: DiffView) => void;
  onClose: () => void;
  onOpenSettings: () => void;
  onImport: () => void;
};

export function PrHeaderBar({
  info,
  view,
  ciStatus,
  onViewChange,
  onClose,
  onOpenSettings,
  onImport,
}: PrHeaderBarProps) {
  return (
    <header className="flex h-11 shrink-0 items-center gap-2 border-b bg-background px-2">
      <IconButton label="Close pull request" onClick={onClose}>
        <RiArrowLeftSLine aria-hidden />
      </IconButton>

      <div className="flex min-w-0 flex-1 items-center gap-2">
        <span className="shrink-0 font-medium font-mono text-sm">
          {info.owner}/{info.repository} #{info.pullNumber}
        </span>
        <span className="truncate text-muted-foreground text-sm">
          {info.title}
        </span>
        <PrStateBadge info={info} />
        <CiStatusChip status={ciStatus} />
        {ciStatus?.mergeable === false ? (
          <Tooltip>
            <TooltipTrigger
              render={
                <Badge className="shrink-0 gap-1" variant="destructive">
                  <RiErrorWarningLine aria-hidden />
                  Conflicts
                </Badge>
              }
            />
            <TooltipContent>
              {ciStatus.mergeableState
                ? `Mergeable state: ${ciStatus.mergeableState}`
                : "Conflicts with the base branch"}
            </TooltipContent>
          </Tooltip>
        ) : null}
        <RefsBadge info={info} />
      </div>

      <ViewToggle onViewChange={onViewChange} view={view} />

      <IconButton label="Import my review comments" onClick={onImport}>
        <RiDownloadLine aria-hidden />
      </IconButton>

      <IconButton label="Settings" onClick={onOpenSettings}>
        <RiSettings4Line aria-hidden />
      </IconButton>
    </header>
  );
}

function prStateLabel(state: string): "Merged" | "Closed" | "Open" {
  if (state === "merged") {
    return "Merged";
  }
  if (state === "closed") {
    return "Closed";
  }
  return "Open";
}

function PrStateBadge({ info }: { info: PullRequestInfo }) {
  const state = info.state.toLowerCase();
  const merged = state === "merged";
  const variant = merged || state === "closed" ? "outline" : "secondary";
  const label = prStateLabel(state);
  return (
    <span className="flex shrink-0 items-center gap-1">
      <Badge variant={variant}>
        {merged ? <RiGitMergeLine aria-hidden /> : null}
        {label}
      </Badge>
      {info.draft ? <Badge variant="outline">Draft</Badge> : null}
    </span>
  );
}

function RefsBadge({ info }: { info: PullRequestInfo }) {
  return (
    <span className="hidden shrink-0 items-center gap-1 rounded-md bg-muted px-1.5 py-0.5 font-mono text-muted-foreground text-xs sm:flex">
      <span className="max-w-32 truncate">{info.baseRef}</span>
      <RiArrowLeftLine aria-hidden className="size-3" />
      <span className="max-w-32 truncate">{info.headRef}</span>
    </span>
  );
}

const CHIP_CLASS =
  "flex shrink-0 items-center gap-1 rounded-md border px-1.5 py-0.5 font-medium text-xs transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

function CiStatusChip({ status }: { status: PrCiStatus | null }) {
  const tone = ciTone(status);
  const summary = summarizeChecks(status?.checkRuns ?? []);

  if (tone === "unknown") {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <span className={cn(CHIP_CLASS, "text-muted-foreground")}>
              <RiSubtractLine aria-hidden className="size-3" />
              Checks
            </span>
          }
        />
        <TooltipContent>
          Check status is unavailable — the Checks permission is missing or no
          checks are configured.
        </TooltipContent>
      </Tooltip>
    );
  }

  return (
    <Popover>
      <PopoverTrigger
        render={
          <button className={cn(CHIP_CLASS, "hover:bg-muted")} type="button">
            <CiChipContent summary={summary} tone={tone} />
          </button>
        }
      />
      <PopoverContent align="start" className="w-80 p-0">
        <div className="border-b px-3 py-2 font-medium text-sm">Checks</div>
        {summary.total === 0 ? (
          <p className="px-3 py-3 text-muted-foreground text-sm">
            No checks reported for this commit.
          </p>
        ) : (
          <div className="flex max-h-80 flex-col overflow-auto py-1">
            {(status?.checkRuns ?? []).map((run) => (
              <CheckRunRow key={`${run.name}:${run.detailsUrl}`} run={run} />
            ))}
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}

function CiChipContent({
  tone,
  summary,
}: {
  tone: "pending" | "failure" | "success";
  summary: ReturnType<typeof summarizeChecks>;
}) {
  if (tone === "pending") {
    return (
      <span className="flex items-center gap-1 text-muted-foreground">
        <Spinner className="size-3" />
        Checks running
      </span>
    );
  }
  if (tone === "failure") {
    return (
      <span className="flex items-center gap-1 text-destructive">
        <RiCloseLine aria-hidden className="size-3" />
        {summary.failing} failing
      </span>
    );
  }
  return (
    <span className="flex items-center gap-1 text-emerald-600 dark:text-emerald-400">
      <RiCheckLine aria-hidden className="size-3" />
      {summary.total} checks
    </span>
  );
}

function CheckRunRow({ run }: { run: CheckRunInfo }) {
  const label =
    run.status === "completed" ? (run.conclusion ?? "completed") : run.status;
  return (
    <div className="flex items-center gap-2 px-3 py-1.5 text-sm">
      <span className="min-w-0 flex-1 truncate">{run.name}</span>
      <Badge className="shrink-0" variant="outline">
        {label}
      </Badge>
      {run.detailsUrl ? (
        <Button
          aria-label={`Open ${run.name} on GitHub`}
          onClick={() => {
            if (run.detailsUrl) {
              openUrl(run.detailsUrl).catch(() => {
                // Best-effort open.
              });
            }
          }}
          size="icon-xs"
          type="button"
          variant="ghost"
        >
          <RiExternalLinkLine aria-hidden />
        </Button>
      ) : null}
    </div>
  );
}

function BranchBadge({ info }: { info: RepositoryInfo }) {
  let label = info.currentBranch ?? "HEAD";
  if (info.detached) {
    label = info.headSha
      ? `detached @ ${info.headSha.slice(0, 7)}`
      : "detached";
  } else if (info.unborn) {
    label = `${label} (unborn)`;
  }
  return (
    <span className="flex min-w-0 shrink items-center gap-1 rounded-md bg-muted px-1.5 py-0.5 font-mono text-muted-foreground text-xs">
      <RiGitBranchLine aria-hidden className="size-3 shrink-0" />
      <span className="truncate">{label}</span>
    </span>
  );
}

function HeadSelector({
  info,
  mode,
  onModeChange,
}: {
  info: RepositoryInfo;
  mode: ComparisonMode;
  onModeChange: (mode: ComparisonMode) => void;
}) {
  const selectedHead =
    mode.kind === "branch" && mode.head
      ? mode.head
      : (info.currentBranch ?? "HEAD");

  const selectHead = (next: string) => {
    if (next === info.currentBranch) {
      if (mode.kind === "branch") {
        onModeChange({ kind: "branch", base: mode.base });
      }
      return;
    }
    const base =
      mode.kind === "branch"
        ? mode.base
        : (info.defaultBaseBranch ?? info.currentBranch ?? "HEAD");
    onModeChange({ kind: "branch", base, head: next });
  };

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger
          render={
            <DropdownMenuTrigger
              render={
                <button
                  className="flex min-w-0 shrink items-center gap-1 rounded-md bg-muted px-1.5 py-0.5 font-mono text-muted-foreground text-xs transition-colors hover:bg-muted/70 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  type="button"
                >
                  <RiGitBranchLine aria-hidden className="size-3 shrink-0" />
                  <span className="truncate">{selectedHead}</span>
                  <RiArrowDownSLine aria-hidden className="size-3 shrink-0" />
                </button>
              }
            />
          }
        />
        <TooltipContent>
          Switch the branch under review — never touches your checkout
        </TooltipContent>
      </Tooltip>
      <DropdownMenuContent
        align="start"
        className="max-h-80 w-64 overflow-auto"
      >
        {/* Base UI GroupLabel throws outside a (Radio)Group, so the label
            lives inside the radio group. */}
        <DropdownMenuRadioGroup onValueChange={selectHead} value={selectedHead}>
          <DropdownMenuLabel>Review branch</DropdownMenuLabel>
          {info.branches.map((branch) => (
            <DropdownMenuRadioItem key={branch} title={branch} value={branch}>
              <span className="truncate font-mono text-xs">{branch}</span>
              {branch === info.currentBranch ? (
                <span
                  className={cn(
                    "ml-auto shrink-0 pl-2 font-mono text-[10px] text-muted-foreground",
                    // Rows reserve pr-8 for the selection check; when this row
                    // isn't selected, reclaim it so HEAD sits flush right.
                    branch !== selectedHead && "-mr-6"
                  )}
                  title="Checked out"
                >
                  HEAD
                </span>
              ) : null}
            </DropdownMenuRadioItem>
          ))}
          {info.remoteBranches.length > 0 ? (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuLabel>Remote branches</DropdownMenuLabel>
              {info.remoteBranches.map((branch) => (
                <DropdownMenuRadioItem
                  key={branch}
                  title={branch}
                  value={branch}
                >
                  <span className="truncate font-mono text-xs">{branch}</span>
                </DropdownMenuRadioItem>
              ))}
            </>
          ) : null}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function ComparisonSelector({
  info,
  mode,
  onModeChange,
}: {
  info: RepositoryInfo;
  mode: ComparisonMode;
  onModeChange: (mode: ComparisonMode) => void;
}) {
  const value = mode.kind === "branch" ? mode.base : WORKING_TREE_VALUE;
  const base = mode.kind === "branch" ? mode.base : "Working tree";
  // Another branch has no working tree, so the option only applies when the
  // reviewed head is the checkout itself.
  const headOverridden = mode.kind === "branch" && mode.head != null;

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger
          render={
            <DropdownMenuTrigger
              render={
                <button
                  className="flex min-w-0 shrink-0 items-center gap-1 rounded-md bg-muted px-1.5 py-0.5 font-mono text-muted-foreground text-xs transition-colors hover:bg-muted/70 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  type="button"
                >
                  <span className="truncate">{base}</span>
                  <RiArrowDownSLine aria-hidden className="size-3 shrink-0" />
                </button>
              }
            />
          }
        />
        <TooltipContent>Change what you compare against</TooltipContent>
      </Tooltip>
      <DropdownMenuContent
        align="start"
        className="max-h-80 w-64 overflow-auto"
      >
        <DropdownMenuRadioGroup
          onValueChange={(next) => {
            if (next === WORKING_TREE_VALUE) {
              onModeChange({ kind: "working-tree" });
            } else {
              onModeChange({
                kind: "branch",
                base: next,
                head: mode.kind === "branch" ? mode.head : undefined,
              });
            }
          }}
          value={value}
        >
          {headOverridden ? null : (
            <DropdownMenuRadioItem value={WORKING_TREE_VALUE}>
              Working tree
            </DropdownMenuRadioItem>
          )}
          {info.branches.length > 0 ? (
            <>
              {headOverridden ? null : <DropdownMenuSeparator />}
              <DropdownMenuLabel>Compare against branch</DropdownMenuLabel>
              {info.branches.map((branch) => (
                <DropdownMenuRadioItem
                  key={branch}
                  title={branch}
                  value={branch}
                >
                  <span className="truncate font-mono text-xs">{branch}</span>
                </DropdownMenuRadioItem>
              ))}
            </>
          ) : null}
          {info.remoteBranches.length > 0 ? (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuLabel>Remote branches</DropdownMenuLabel>
              {info.remoteBranches.map((branch) => (
                <DropdownMenuRadioItem
                  key={branch}
                  title={branch}
                  value={branch}
                >
                  <span className="truncate font-mono text-xs">{branch}</span>
                </DropdownMenuRadioItem>
              ))}
            </>
          ) : null}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function ViewToggle({
  view,
  onViewChange,
}: {
  view: DiffView;
  onViewChange: (view: DiffView) => void;
}) {
  return (
    <div className="flex items-center gap-0.5">
      <SegmentButton
        active={view === "split"}
        label="Split view"
        onClick={() => onViewChange("split")}
      >
        <RiLayoutColumnLine aria-hidden />
      </SegmentButton>
      <SegmentButton
        active={view === "unified"}
        label="Unified view"
        onClick={() => onViewChange("unified")}
      >
        <RiLayoutRowLine aria-hidden />
      </SegmentButton>
    </div>
  );
}

function SegmentButton({
  active,
  label,
  onClick,
  children,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        aria-label={label}
        aria-pressed={active}
        className={cn(
          "flex size-7 items-center justify-center rounded-lg transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&_svg]:size-4",
          active
            ? "bg-muted text-foreground"
            : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
        )}
        onClick={onClick}
        render={<button type="button" />}
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function IconButton({
  label,
  onClick,
  pending,
  children,
}: {
  label: string;
  onClick: () => void;
  pending?: boolean;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            aria-label={label}
            disabled={pending}
            onClick={onClick}
            size="icon-sm"
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
