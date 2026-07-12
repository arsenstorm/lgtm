import { invoke } from "@tauri-apps/api/core";
import type {
  DeviceFlowStart,
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

/** Begin the GitHub device flow. Pass a user-supplied client ID, or null to
 * fall back to a baked-in registration on the Rust side. */
export async function startDeviceFlow(
  clientId: string | null
): Promise<DeviceFlowStart> {
  try {
    return await invoke<DeviceFlowStart>("github_device_start", { clientId });
  } catch (e) {
    throw toAppError(e);
  }
}

/** Resolves with the GitHub login once the user approves in the browser. */
export async function waitDeviceFlow(): Promise<string> {
  try {
    return await invoke<string>("github_device_wait", {});
  } catch (e) {
    throw toAppError(e);
  }
}

export async function cancelDeviceFlow(): Promise<void> {
  try {
    await invoke<void>("github_device_cancel", {});
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
