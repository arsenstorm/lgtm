import { useRouter } from "@tanstack/react-router";
import { useCallback, useState } from "react";
import { toast } from "sonner";

/**
 * The one lifecycle every mutating page action shares: mark the action
 * pending, run the call, toast the outcome, refetch the route. Resolves to
 * whether the call succeeded, so callers clear local state on success only.
 */
export function useAction<A extends string = string>(options?: {
  /** Runs as any action starts; the armed-confirm pages disarm here. */
  onStart?: () => void;
}) {
  const router = useRouter();
  const [pending, setPending] = useState<A | null>(null);
  const onStart = options?.onStart;

  const run = useCallback(
    async (action: A, call: () => Promise<unknown>, message: string) => {
      setPending(action);
      onStart?.();
      try {
        await call();
        toast.success(message);
        await router.invalidate();
        return true;
      } catch (error) {
        // The orchestrator's refusal reason is the whole message; genericising
        // it would throw away the only thing that says what to do next.
        toast.error(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        setPending(null);
      }
    },
    [router, onStart]
  );

  return { busy: pending !== null, pending, run };
}
