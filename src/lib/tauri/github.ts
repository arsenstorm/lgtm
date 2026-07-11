import { invoke } from "@tauri-apps/api/core";
import type {
  GithubPrBundle,
  GithubReviewCommentDraft,
  GithubReviewEvent,
  ImportPage,
  SubmittedReview,
} from "../../types/github";
import { toAppError } from "../errors/app-error";

export async function setGithubToken(token: string): Promise<string> {
  try {
    return await invoke<string>("github_set_token", { token });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function getGithubTokenStatus(): Promise<string | null> {
  try {
    return await invoke<string | null>("github_token_status");
  } catch (e) {
    throw toAppError(e);
  }
}

export async function clearGithubToken(): Promise<void> {
  try {
    await invoke<void>("github_clear_token");
  } catch (e) {
    throw toAppError(e);
  }
}

export async function openGithubPr(url: string): Promise<GithubPrBundle> {
  try {
    return await invoke<GithubPrBundle>("github_open_pr", { url });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function submitGithubReview(args: {
  owner: string;
  repository: string;
  pullNumber: number;
  expectedHeadSha: string;
  event: GithubReviewEvent;
  body: string;
  comments: GithubReviewCommentDraft[];
}): Promise<SubmittedReview> {
  try {
    return await invoke<SubmittedReview>("github_submit_review", { args });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function importGithubReviewComments(
  owner: string,
  repository: string,
  page: number
): Promise<ImportPage> {
  try {
    return await invoke<ImportPage>("github_import_review_comments", {
      owner,
      repository,
      page,
    });
  } catch (e) {
    throw toAppError(e);
  }
}
