import { invoke } from "@tauri-apps/api/core";
import type {
  ConversationComment,
  DeviceFlowStart,
  GithubPrBundle,
  GithubReviewCommentDraft,
  GithubReviewEvent,
  ImportPage,
  MergeMethod,
  MergeResult,
  PrCiStatus,
  PrInlineComment,
  PullRequestSummary,
  ReviewInfo,
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

export async function listPullRequests(
  owner: string,
  repository: string
): Promise<PullRequestSummary[]> {
  try {
    return await invoke<PullRequestSummary[]>("github_list_pull_requests", {
      owner,
      repository,
    });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function mergePullRequest(args: {
  owner: string;
  repository: string;
  pullNumber: number;
  expectedHeadSha: string;
  method: MergeMethod;
  commitTitle: string | null;
  commitMessage: string | null;
  deleteBranch: boolean;
}): Promise<MergeResult> {
  try {
    return await invoke<MergeResult>("github_merge_pr", { args });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function setPullRequestState(
  owner: string,
  repository: string,
  pullNumber: number,
  state: "open" | "closed"
): Promise<string> {
  try {
    return await invoke<string>("github_set_pr_state", {
      owner,
      repository,
      pullNumber,
      state,
    });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function listReviews(
  owner: string,
  repository: string,
  pullNumber: number
): Promise<ReviewInfo[]> {
  try {
    return await invoke<ReviewInfo[]>("github_list_reviews", {
      owner,
      repository,
      pullNumber,
    });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function dismissReview(
  owner: string,
  repository: string,
  pullNumber: number,
  reviewId: number,
  message: string
): Promise<void> {
  try {
    await invoke<void>("github_dismiss_review", {
      owner,
      repository,
      pullNumber,
      reviewId,
      message,
    });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function listPrInlineComments(
  owner: string,
  repository: string,
  pullNumber: number
): Promise<PrInlineComment[]> {
  try {
    return await invoke<PrInlineComment[]>("github_list_pr_comments", {
      owner,
      repository,
      pullNumber,
    });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function deleteReviewComment(
  owner: string,
  repository: string,
  commentId: number
): Promise<void> {
  try {
    await invoke<void>("github_delete_review_comment", {
      owner,
      repository,
      commentId,
    });
  } catch (e) {
    throw toAppError(e);
  }
}

export async function addConversationComment(
  owner: string,
  repository: string,
  pullNumber: number,
  body: string
): Promise<ConversationComment> {
  try {
    return await invoke<ConversationComment>(
      "github_add_conversation_comment",
      { owner, repository, pullNumber, body }
    );
  } catch (e) {
    throw toAppError(e);
  }
}

export async function listConversationComments(
  owner: string,
  repository: string,
  pullNumber: number
): Promise<ConversationComment[]> {
  try {
    return await invoke<ConversationComment[]>(
      "github_list_conversation_comments",
      { owner, repository, pullNumber }
    );
  } catch (e) {
    throw toAppError(e);
  }
}

export async function getPrCiStatus(
  owner: string,
  repository: string,
  pullNumber: number
): Promise<PrCiStatus> {
  try {
    return await invoke<PrCiStatus>("github_pr_ci_status", {
      owner,
      repository,
      pullNumber,
    });
  } catch (e) {
    throw toAppError(e);
  }
}
