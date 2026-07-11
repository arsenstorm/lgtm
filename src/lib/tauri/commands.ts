import { invoke } from "@tauri-apps/api/core";
import type {
  DiffResult,
  DiffSourceArgs,
  RepositoryInfo,
} from "../../types/git";
import { toAppError } from "../errors/app-error";

export async function openRepository(path: string): Promise<RepositoryInfo> {
  try {
    return await invoke<RepositoryInfo>("open_repository", { path });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function getDiff(
  repoPath: string,
  source: DiffSourceArgs
): Promise<DiffResult> {
  try {
    return await invoke<DiffResult>("get_diff", { repoPath, source });
  } catch (e) {
    throw toAppError(e);
  }
}
