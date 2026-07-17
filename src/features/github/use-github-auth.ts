import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";
import { getSetting } from "@/lib/db/settings";
import { type AppError, toAppError } from "@/lib/errors/app-error";
import {
  cancelDeviceFlow,
  clearGithubToken,
  getGithubTokenStatus,
  startDeviceFlow,
  waitDeviceFlow,
} from "@/lib/tauri/github";

export const GITHUB_CLIENT_ID_KEY = "github-client-id";

export type GithubAuthStatus =
  | "idle"
  | "starting"
  | "awaiting-approval"
  | "connected";

export type DeviceInfo = { userCode: string; verificationUri: string };

/**
 * Drives the GitHub device-flow connection. `start` kicks off the flow and then
 * waits for browser approval in the same call; `cancel` (or closing the dialog)
 * aborts an in-flight attempt. Each attempt carries a monotonic id so a wait
 * that resolves/rejects after the user walked away is silently discarded — this
 * is how a user cancel stays quiet while a genuine expiry/decline surfaces.
 */
export function useGithubAuth() {
  const [status, setStatus] = useState<GithubAuthStatus>("idle");
  const [login, setLogin] = useState<string | null>(null);
  const [device, setDevice] = useState<DeviceInfo | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [needsClientId, setNeedsClientId] = useState(false);
  const attemptRef = useRef(0);

  const refresh = useCallback(async () => {
    try {
      const current = await getGithubTokenStatus();
      setLogin(current);
      setStatus(current ? "connected" : "idle");
    } catch {
      setStatus("idle");
    }
  }, []);

  const start = useCallback(async () => {
    const attempt = ++attemptRef.current;
    setError(null);
    setNeedsClientId(false);
    setDevice(null);
    setStatus("starting");

    try {
      const clientId = await getSetting(GITHUB_CLIENT_ID_KEY);
      const flow = await startDeviceFlow(clientId);
      if (attemptRef.current !== attempt) {
        return;
      }
      setDevice({
        userCode: flow.userCode,
        verificationUri: flow.verificationUri,
      });
      setStatus("awaiting-approval");
    } catch (e) {
      if (attemptRef.current !== attempt) {
        return;
      }
      const err = toAppError(e);
      // startDeviceFlow's only invalid-argument is the missing-client-ID case.
      setNeedsClientId(err.code === "invalid-argument");
      setError(err);
      setStatus("idle");
      return;
    }

    try {
      const user = await waitDeviceFlow();
      if (attemptRef.current !== attempt) {
        return;
      }
      setLogin(user);
      setDevice(null);
      setStatus("connected");
      toast.success(`Connected as ${user}`);
    } catch (e) {
      // A newer attempt (or a cancel) invalidated this wait — stay silent.
      if (attemptRef.current !== attempt) {
        return;
      }
      setError(toAppError(e));
      setDevice(null);
      setStatus("idle");
    }
  }, []);

  const cancel = useCallback(async () => {
    attemptRef.current++;
    setDevice(null);
    setError(null);
    setStatus((s) => (s === "connected" ? s : "idle"));
    try {
      await cancelDeviceFlow();
    } catch {
      // Best-effort: the flow may already be finished on the Rust side.
    }
  }, []);

  const clearError = useCallback(() => {
    setError(null);
    setNeedsClientId(false);
  }, []);

  const disconnect = useCallback(async () => {
    await clearGithubToken();
    setLogin(null);
    setStatus("idle");
    setError(null);
  }, []);

  return {
    status,
    login,
    device,
    error,
    needsClientId,
    start,
    cancel,
    refresh,
    disconnect,
    clearError,
  };
}
