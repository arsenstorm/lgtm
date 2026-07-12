import type { SelectedLineRange } from "@pierre/diffs/react";
import { useTheme } from "next-themes";
import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from "react";
import { toast } from "sonner";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ChangedFileList } from "@/features/changes/changed-file-list";
import {
  type ComparisonMode,
  describeComparison,
} from "@/features/changes/comparison";
import { type DiffData, useDiff } from "@/features/changes/use-diff";
import { useFileReview } from "@/features/changes/use-file-review";
import { useReviewSession } from "@/features/changes/use-review-session";
import { type DiffView, DiffViewer } from "@/features/diff/diff-viewer";
import { ImportDialog } from "@/features/github/import-dialog";
import { OpenPrDialog } from "@/features/github/open-pr-dialog";
import { PrBrowserDialog } from "@/features/github/pr-browser-dialog";
import type { SubmitContext } from "@/features/github/submit-review";
import { TokenDialog } from "@/features/github/token-dialog";
import { usePrDiff, usePrSession } from "@/features/github/use-pr-workspace";
import { useSuggestions } from "@/features/memory/use-suggestions";
import { RepositoryPicker } from "@/features/repositories/repository-picker";
import {
  type ActiveSource,
  useRepository,
} from "@/features/repositories/use-repository";
import { CommandPalette } from "@/features/reviews/command-palette";
import { ReviewSummary } from "@/features/reviews/review-summary";
import { useReviewComments } from "@/features/reviews/use-review-comments";
import {
  type ReviewAction,
  useReviewShortcuts,
} from "@/features/reviews/use-review-shortcuts";
import { buildAnchor } from "@/lib/diff/anchor";
import type { AppError } from "@/lib/errors/app-error";
import { parseGithubRemote } from "@/lib/github/remote";
import { GITHUB_PATH_PREFIX } from "@/lib/github/repo-identity";
import type { RepositoryRecord, ReviewComment } from "@/types/review";
import { HeaderBar, PrHeaderBar } from "./header-bar";
import { StatusBar } from "./status-bar";

const FLASH_MS = 2000;

export function AppShell() {
  const repo = useRepository();
  const [tokenOpen, setTokenOpen] = useState(false);
  const [prOpen, setPrOpen] = useState(false);
  const [prPrefill, setPrPrefill] = useState("");
  const [prBrowserOpen, setPrBrowserOpen] = useState(false);

  const openPrDialog = useCallback((prefill = "") => {
    setPrPrefill(prefill);
    setPrOpen(true);
  }, []);
  const openTokenDialog = useCallback(() => setTokenOpen(true), []);
  const openPrBrowser = useCallback(() => {
    setPrOpen(false);
    setPrBrowserOpen(true);
  }, []);

  // The PR browser only makes sense for a local repo with a GitHub remote.
  const activeLocal = repo.active?.kind === "local" ? repo.active : null;
  const localRemote = activeLocal
    ? parseGithubRemote(activeLocal.info.remoteUrl)
    : null;

  const { openPath } = repo;
  const onOpenRecent = useCallback(
    (record: RepositoryRecord) => {
      if (record.path.startsWith(GITHUB_PATH_PREFIX)) {
        // github:// records aren't real paths; reopen via the PR dialog. We
        // don't persist PR numbers, so prefill only up to /pull/.
        const slug = record.path.slice(GITHUB_PATH_PREFIX.length);
        openPrDialog(`https://github.com/${slug}/pull/`);
        return;
      }
      openPath(record.path);
    },
    [openPath, openPrDialog]
  );

  return (
    <>
      <ActiveView
        active={repo.active}
        error={repo.error}
        onBrowsePrs={localRemote ? openPrBrowser : undefined}
        onClose={repo.close}
        onDismissError={repo.dismissError}
        onManageToken={openTokenDialog}
        onOpenPicker={repo.openFromPicker}
        onOpenPr={() => openPrDialog()}
        onOpenRecent={onOpenRecent}
        opening={repo.opening}
        recents={repo.recents}
        recentsLoading={repo.recentsLoading}
      />
      <TokenDialog onOpenChange={setTokenOpen} open={tokenOpen} />
      <OpenPrDialog
        onBrowsePrs={localRemote ? openPrBrowser : undefined}
        onManageToken={openTokenDialog}
        onOpen={repo.openPr}
        onOpenChange={setPrOpen}
        open={prOpen}
        opening={repo.opening}
        prefillUrl={prPrefill}
      />
      {localRemote ? (
        <PrBrowserDialog
          onManageToken={openTokenDialog}
          onOpen={repo.openPr}
          onOpenChange={setPrBrowserOpen}
          open={prBrowserOpen}
          opening={repo.opening}
          owner={localRemote.owner}
          repository={localRemote.repository}
        />
      ) : null}
    </>
  );
}

function ActiveView({
  active,
  onBrowsePrs,
  onClose,
  onManageToken,
  onOpenPr,
  ...pickerProps
}: {
  active: ActiveSource | null;
  onBrowsePrs?: () => void;
  onClose: () => void;
  onManageToken: () => void;
  onOpenPr: () => void;
  recents: RepositoryRecord[];
  recentsLoading: boolean;
  opening: boolean;
  error: AppError | null;
  onOpenPicker: () => void;
  onOpenRecent: (record: RepositoryRecord) => void;
  onDismissError: () => void;
}) {
  if (!active) {
    return (
      <RepositoryPicker
        {...pickerProps}
        onManageToken={onManageToken}
        onOpenPr={onOpenPr}
      />
    );
  }

  // Keyed by record id so all workspace state resets cleanly on target change.
  if (active.kind === "github-pr") {
    return (
      <PrReviewWorkspace
        active={active}
        key={active.record.id}
        onClose={onClose}
        onManageToken={onManageToken}
        onOpenPrDialog={onOpenPr}
      />
    );
  }
  return (
    <LocalReviewWorkspace
      active={active}
      key={active.record.id}
      onBrowsePrs={onBrowsePrs}
      onClose={onClose}
      onManageToken={onManageToken}
      onOpenPrDialog={onOpenPr}
    />
  );
}

type DiffController = {
  data: DiffData | null;
  loading: boolean;
  refreshing: boolean;
  error: AppError | null;
  refresh: () => void;
};

function LocalReviewWorkspace({
  active,
  onBrowsePrs,
  onClose,
  onManageToken,
  onOpenPrDialog,
}: {
  active: Extract<ActiveSource, { kind: "local" }>;
  onBrowsePrs?: () => void;
  onClose: () => void;
  onManageToken: () => void;
  onOpenPrDialog: () => void;
}) {
  const { info, record } = active;
  const [mode, setMode] = useState<ComparisonMode>({ kind: "working-tree" });
  const session = useReviewSession({
    repositoryId: record.id,
    mode,
    headRevision: info.currentBranch ?? "HEAD",
  });
  const diff = useDiff({
    rootPath: info.rootPath,
    mode,
    sessionId: session?.id ?? null,
  });
  const comparisonKey =
    mode.kind === "branch" ? `branch:${mode.base}` : "working-tree";

  return (
    <ReviewWorkspaceBody
      comparisonKey={comparisonKey}
      comparisonLabel={describeComparison(mode)}
      diff={diff}
      onBrowsePrs={onBrowsePrs}
      onManageToken={onManageToken}
      onOpenPrDialog={onOpenPrDialog}
      record={record}
      renderHeader={(view, onViewChange) => (
        <HeaderBar
          info={info}
          mode={mode}
          onBrowsePrs={onBrowsePrs}
          onClose={onClose}
          onManageToken={onManageToken}
          onModeChange={setMode}
          onRefresh={diff.refresh}
          onViewChange={onViewChange}
          refreshing={diff.refreshing}
          view={view}
        />
      )}
      repoName={info.displayName}
      session={session}
      statusHeadSha={info.headSha}
    />
  );
}

function PrReviewWorkspace({
  active,
  onClose,
  onManageToken,
  onOpenPrDialog,
}: {
  active: Extract<ActiveSource, { kind: "github-pr" }>;
  onClose: () => void;
  onManageToken: () => void;
  onOpenPrDialog: () => void;
}) {
  const { bundle, record } = active;
  const { info } = bundle;
  const [importOpen, setImportOpen] = useState(false);
  const openImport = useCallback(() => setImportOpen(true), []);

  const session = usePrSession({ repositoryId: record.id, info });
  const diff = usePrDiff({
    url: info.htmlUrl,
    patch: bundle.patch,
    baseSha: info.baseSha,
    headSha: info.headSha,
    sessionId: session?.id ?? null,
  });
  const repoName = `${info.owner}/${info.repository}`;

  return (
    <>
      <ReviewWorkspaceBody
        comparisonKey={`pr:${info.pullNumber}`}
        comparisonLabel={`${repoName} #${info.pullNumber}`}
        diff={diff}
        onImport={openImport}
        onManageToken={onManageToken}
        onOpenPrDialog={onOpenPrDialog}
        record={record}
        renderHeader={(view, onViewChange) => (
          <PrHeaderBar
            info={info}
            onClose={onClose}
            onImport={openImport}
            onManageToken={onManageToken}
            onRefresh={diff.refresh}
            onViewChange={onViewChange}
            refreshing={diff.refreshing}
            view={view}
          />
        )}
        repoName={repoName}
        session={session}
        statusHeadSha={info.headSha}
        submitBase={{
          owner: info.owner,
          repository: info.repository,
          pullNumber: info.pullNumber,
        }}
      />
      <ImportDialog
        onOpenChange={setImportOpen}
        open={importOpen}
        owner={info.owner}
        repository={info.repository}
        repositoryId={record.id}
      />
    </>
  );
}

type ReviewWorkspaceBodyProps = {
  record: RepositoryRecord;
  session: ReturnType<typeof useReviewSession>;
  diff: DiffController;
  repoName: string;
  comparisonLabel: string;
  comparisonKey: string;
  statusHeadSha: string | null;
  renderHeader: (
    view: DiffView,
    onViewChange: (view: DiffView) => void
  ) => ReactNode;
  onManageToken: () => void;
  onOpenPrDialog: () => void;
  onBrowsePrs?: () => void;
  onImport?: () => void;
  submitBase?: Pick<SubmitContext, "owner" | "repository" | "pullNumber">;
};

function ReviewWorkspaceBody({
  record,
  session,
  diff,
  repoName,
  comparisonLabel,
  comparisonKey,
  statusHeadSha,
  renderHeader,
  onManageToken,
  onOpenPrDialog,
  onBrowsePrs,
  onImport,
  submitBase,
}: ReviewWorkspaceBodyProps) {
  const { resolvedTheme } = useTheme();
  const theme = resolvedTheme === "dark" ? "dark" : "light";

  const [view, setView] = useState<DiffView>("split");
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [selection, setSelection] = useState<SelectedLineRange | null>(null);
  const [composerOpen, setComposerOpen] = useState(false);
  const [summaryOpen, setSummaryOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [activeCommentId, setActiveCommentId] = useState<string | null>(null);

  const review = useFileReview(session?.id ?? null);
  const comments = useReviewComments({
    sessionId: session?.id ?? null,
    repositoryId: record.id,
    diffData: diff.data,
  });
  const suggestions = useSuggestions({
    sessionId: session?.id ?? null,
    repositoryId: record.id,
    diffData: diff.data,
    comments: comments.comments,
    createComment: comments.create,
  });

  const files = diff.data?.files ?? [];
  const untracked = diff.data?.untracked ?? [];

  // Keep a valid selection: default to the first file, drop stale selections.
  useEffect(() => {
    if (files.length === 0) {
      setSelectedFile(null);
      return;
    }
    setSelectedFile((current) => {
      if (current && files.some((file) => file.name === current)) {
        return current;
      }
      return files[0].name;
    });
  }, [files]);

  // Selection + composer are tied to the file on screen; reset them on switch.
  // biome-ignore lint/correctness/useExhaustiveDependencies: selectedFile and comparisonKey are intentional triggers, not values read in the effect.
  useEffect(() => {
    setSelection(null);
    setComposerOpen(false);
  }, [selectedFile, comparisonKey]);

  // A flashed comment highlight fades on its own.
  useEffect(() => {
    if (!activeCommentId) {
      return;
    }
    const timer = window.setTimeout(() => setActiveCommentId(null), FLASH_MS);
    return () => window.clearTimeout(timer);
  }, [activeCommentId]);

  const selected = files.find((file) => file.name === selectedFile) ?? null;
  const viewedCount = files.filter((file) =>
    review.viewed.has(file.name)
  ).length;
  const outdatedTotal = comments.comments.filter(
    (comment) => comment.status === "outdated"
  ).length;
  const fileComments = selected
    ? (comments.byFile.get(selected.name) ?? [])
    : [];
  const fileSuggestions = selected
    ? (suggestions.byFile.get(selected.name) ?? [])
    : [];

  const saveComment = useCallback(
    async (body: string) => {
      if (!(selected && selection && session)) {
        return;
      }
      const side = selection.side ?? "additions";
      const anchor = buildAnchor({
        file: selected,
        side,
        startLine: selection.start,
        endLine: selection.end,
        baseRevision: diff.data?.baseSha ?? "",
        headRevision: diff.data?.headSha ?? "",
      });
      if (!anchor) {
        toast.error("This range can't be commented", {
          description: "The selection crosses a collapsed region or hunk.",
        });
        return;
      }
      await comments.create(anchor, body);
      setComposerOpen(false);
      setSelection(null);
    },
    [selected, selection, session, diff.data, comments]
  );

  const openComposer = useCallback(() => {
    if (!selection) {
      toast("Select lines first", {
        description: "Drag across the gutter, then press c.",
      });
      return;
    }
    if (
      selection.side &&
      selection.endSide &&
      selection.side !== selection.endSide
    ) {
      toast.error("Comments can't span both sides of a diff");
      return;
    }
    setComposerOpen(true);
  }, [selection]);

  const navigateToComment = useCallback((comment: ReviewComment) => {
    setSelectedFile(comment.anchor.path);
    setActiveCommentId(comment.id);
    setSummaryOpen(false);
  }, []);

  const stepComment = useCallback(
    (direction: 1 | -1) => {
      const list = comments.ordered;
      if (list.length === 0) {
        toast("No comments yet");
        return;
      }
      const index = list.findIndex((item) => item.id === activeCommentId);
      let target: ReviewComment;
      if (index === -1) {
        target = (direction === 1 ? list[0] : list.at(-1)) ?? list[0];
      } else {
        target = list[(index + direction + list.length) % list.length];
      }
      navigateToComment(target);
    },
    [comments.ordered, activeCommentId, navigateToComment]
  );

  const stepFile = useCallback(
    (direction: 1 | -1) => {
      if (files.length === 0) {
        return;
      }
      const index = files.findIndex((file) => file.name === selectedFile);
      const next =
        index === -1 ? 0 : (index + direction + files.length) % files.length;
      setSelectedFile(files[next].name);
    },
    [files, selectedFile]
  );

  const actions = useMemo<ReviewAction[]>(
    () => [
      {
        id: "next-file",
        label: "Next file",
        hint: ["J"],
        key: "j",
        run: () => stepFile(1),
        disabled: files.length < 2,
      },
      {
        id: "prev-file",
        label: "Previous file",
        hint: ["K"],
        key: "k",
        run: () => stepFile(-1),
        disabled: files.length < 2,
      },
      {
        id: "next-comment",
        label: "Next comment",
        hint: ["N"],
        key: "n",
        run: () => stepComment(1),
        disabled: comments.ordered.length === 0,
      },
      {
        id: "prev-comment",
        label: "Previous comment",
        hint: ["P"],
        key: "p",
        run: () => stepComment(-1),
        disabled: comments.ordered.length === 0,
      },
      {
        id: "comment",
        label: "Comment on selection",
        hint: ["C"],
        key: "c",
        run: openComposer,
        disabled: !selected,
      },
      {
        id: "toggle-viewed",
        label: "Toggle file viewed",
        hint: ["V"],
        key: "v",
        run: () => selected && review.toggle(selected.name),
        disabled: !selected,
      },
      {
        id: "refresh",
        label: "Refresh diff",
        hint: ["R"],
        key: "r",
        run: diff.refresh,
      },
      {
        id: "summary",
        label: "Open review summary",
        hint: ["S"],
        key: "s",
        run: () => setSummaryOpen(true),
      },
      {
        id: "palette",
        label: "Command palette",
        hint: ["⌘", "K"],
        key: "k",
        meta: true,
        run: () => setPaletteOpen(true),
      },
    ],
    [
      files.length,
      comments.ordered.length,
      selected,
      openComposer,
      stepFile,
      stepComment,
      review,
      diff.refresh,
    ]
  );

  useReviewShortcuts(actions);

  // GitHub actions live only in the palette (no shortcut). Submit + import
  // appear only in PR mode.
  const paletteActions = useMemo<ReviewAction[]>(() => {
    const extras: ReviewAction[] = [];
    if (submitBase) {
      extras.push({
        id: "submit-review",
        label: "Submit review…",
        hint: [],
        key: "",
        run: () => setSummaryOpen(true),
      });
    }
    if (onImport) {
      extras.push({
        id: "import-comments",
        label: "Import my review comments…",
        hint: [],
        key: "",
        run: onImport,
      });
    }
    if (onBrowsePrs) {
      extras.push({
        id: "browse-prs",
        label: "Browse pull requests…",
        hint: [],
        key: "",
        run: onBrowsePrs,
      });
    }
    extras.push(
      {
        id: "open-pr",
        label: "Open GitHub pull request…",
        hint: [],
        key: "",
        run: onOpenPrDialog,
      },
      {
        id: "github-token",
        label: "Connect to GitHub…",
        hint: [],
        key: "",
        run: onManageToken,
      }
    );
    return [...actions, ...extras];
  }, [
    actions,
    submitBase,
    onImport,
    onBrowsePrs,
    onOpenPrDialog,
    onManageToken,
  ]);

  const submit = useMemo<SubmitContext | undefined>(() => {
    if (!submitBase) {
      return;
    }
    return {
      ...submitBase,
      headSha: diff.data?.headSha ?? statusHeadSha ?? "",
      onPublished: comments.markPublished,
      onRevisionChanged: diff.refresh,
    };
  }, [
    submitBase,
    diff.data?.headSha,
    diff.refresh,
    statusHeadSha,
    comments.markPublished,
  ]);

  return (
    <div className="flex h-dvh flex-col bg-background">
      {renderHeader(view, setView)}

      <ResizablePanelGroup className="flex-1" orientation="horizontal">
        <ResizablePanel defaultSize={26} minSize="180px">
          <ScrollArea className="h-full">
            <ChangedFileList
              commentCounts={comments.counts}
              files={files}
              loading={diff.loading}
              onSelect={setSelectedFile}
              onToggleViewed={review.toggle}
              selectedFile={selectedFile}
              suggestionCounts={suggestions.counts}
              untracked={untracked}
              viewed={review.viewed}
            />
          </ScrollArea>
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel defaultSize={74} minSize={30}>
          <DiffViewer
            activeCommentId={activeCommentId}
            comments={fileComments}
            comparisonKey={comparisonKey}
            comparisonLabel={comparisonLabel}
            composerOpen={composerOpen}
            error={diff.error}
            file={selected}
            hasFiles={files.length > 0}
            loading={diff.loading}
            onAcceptSuggestion={suggestions.accept}
            onCancelComposer={() => setComposerOpen(false)}
            onDeleteComment={comments.remove}
            onDismissSuggestion={suggestions.dismiss}
            onEditAcceptSuggestion={suggestions.editAndAccept}
            onEditComment={comments.edit}
            onNeverAgainSuggestion={suggestions.neverAgain}
            onRetry={diff.refresh}
            onSaveComment={saveComment}
            onSelectionChange={setSelection}
            selection={selection}
            suggestions={fileSuggestions}
            theme={theme}
            view={view}
          />
        </ResizablePanel>
      </ResizablePanelGroup>

      <StatusBar
        changedCount={files.length}
        commentCount={comments.comments.length}
        comparisonLabel={comparisonLabel}
        headSha={diff.data?.headSha ?? statusHeadSha}
        onOpenReview={() => setSummaryOpen(true)}
        outdatedCount={outdatedTotal}
        suggestionCount={suggestions.total}
        untrackedCount={untracked.length}
        viewedCount={viewedCount}
      />

      <ReviewSummary
        byFile={comments.byFile}
        comments={comments.ordered}
        comparisonLabel={comparisonLabel}
        onDelete={comments.remove}
        onEdit={comments.edit}
        onNavigate={navigateToComment}
        onOpenChange={setSummaryOpen}
        open={summaryOpen}
        outdatedTotal={outdatedTotal}
        repoName={repoName}
        submit={submit}
        total={comments.comments.length}
      />

      <CommandPalette
        actions={paletteActions}
        onOpenChange={setPaletteOpen}
        open={paletteOpen}
      />
    </div>
  );
}
