import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import {
  listRecentRepositories,
  upsertRepository,
} from "@/lib/db/repositories";
import { type AppError, toAppError } from "@/lib/errors/app-error";
import { openRepository } from "@/lib/tauri/commands";
import { openGithubPr } from "@/lib/tauri/github";
import type { RepositoryInfo } from "@/types/git";
import type { GithubPrBundle } from "@/types/github";
import type { RepositoryRecord } from "@/types/review";

/** The workspace's active review target: a local repo or a GitHub PR. */
export type ActiveSource =
  | { kind: "local"; info: RepositoryInfo; record: RepositoryRecord }
  | { kind: "github-pr"; bundle: GithubPrBundle; record: RepositoryRecord };

export const GITHUB_PATH_PREFIX = "github://";

type RepositoryState = {
  active: ActiveSource | null;
  recents: RepositoryRecord[];
  recentsLoading: boolean;
  opening: boolean;
  error: AppError | null;
};

export function useRepository() {
  const [state, setState] = useState<RepositoryState>({
    active: null,
    recents: [],
    recentsLoading: true,
    opening: false,
    error: null,
  });

  const refreshRecents = useCallback(async () => {
    try {
      const recents = await listRecentRepositories();
      setState((prev) => ({ ...prev, recents, recentsLoading: false }));
    } catch {
      // Recents are best-effort; a failure here should not block opening.
      setState((prev) => ({ ...prev, recentsLoading: false }));
    }
  }, []);

  useEffect(() => {
    refreshRecents();
  }, [refreshRecents]);

  const openPath = useCallback(
    async (path: string) => {
      setState((prev) => ({ ...prev, opening: true, error: null }));
      try {
        const info = await openRepository(path);
        // upsertRepository also bumps last_opened_at, so it doubles as touch.
        const record = await upsertRepository(info);
        setState((prev) => ({
          ...prev,
          active: { kind: "local", record, info },
          opening: false,
        }));
        await refreshRecents();
      } catch (error) {
        setState((prev) => ({
          ...prev,
          opening: false,
          error: toAppError(error),
        }));
      }
    },
    [refreshRecents]
  );

  const openFromPicker = useCallback(async () => {
    const selected = await open({ directory: true, title: "Open repository" });
    if (typeof selected === "string") {
      await openPath(selected);
    }
  }, [openPath]);

  const openPr = useCallback(
    async (url: string): Promise<AppError | null> => {
      setState((prev) => ({ ...prev, opening: true }));
      try {
        const bundle = await openGithubPr(url);
        const { owner, repository } = bundle.info;
        // Synthetic record so sessions/memory scope to the GitHub repo. Only
        // path/displayName/remoteUrl/defaultBaseBranch are persisted.
        const record = await upsertRepository({
          rootPath: `${GITHUB_PATH_PREFIX}${owner}/${repository}`,
          displayName: `${owner}/${repository}`,
          currentBranch: null,
          headSha: null,
          detached: false,
          unborn: false,
          remoteUrl: `https://github.com/${owner}/${repository}`,
          defaultBaseBranch: null,
          branches: [],
        });
        setState((prev) => ({
          ...prev,
          active: { kind: "github-pr", bundle, record },
          opening: false,
        }));
        await refreshRecents();
        return null;
      } catch (error) {
        // Returned to the dialog for inline display rather than the picker,
        // so an auth failure can point straight at the token dialog.
        setState((prev) => ({ ...prev, opening: false }));
        return toAppError(error);
      }
    },
    [refreshRecents]
  );

  const close = useCallback(() => {
    setState((prev) => ({ ...prev, active: null, error: null }));
    refreshRecents();
  }, [refreshRecents]);

  const dismissError = useCallback(() => {
    setState((prev) => ({ ...prev, error: null }));
  }, []);

  return {
    ...state,
    openFromPicker,
    openPath,
    openPr,
    close,
    dismissError,
  };
}
