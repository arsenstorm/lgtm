import { useCallback, useEffect, useRef, useState } from "react";

/** Long enough to read "Confirm delete", short enough that a forgotten arm
 *  cannot still be live when the next person reaches the keyboard. */
const DISARM_MS = 4000;

/** The variant's own `dark:` classes outrank an unprefixed override, so the
 *  armed fill has to be stated for both themes. */
export const ARMED_CLASS =
  "bg-destructive text-destructive-foreground hover:bg-destructive/90 dark:bg-destructive dark:hover:bg-destructive/90";

/**
 * A two-press destructive button: the first press arms, the second fires.
 * Arming puts the surface in a mode, and a mode nobody meant to enter has to
 * expire on its own: a pointer anywhere else, Escape, or the timeout.
 * `ref` goes on the armed button so its own presses do not disarm it.
 */
export function useArmedConfirm() {
  const [armed, setArmed] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);

  const arm = useCallback(() => setArmed(true), []);
  const disarm = useCallback(() => setArmed(false), []);

  useEffect(() => {
    if (!armed) {
      return;
    }

    const disarmNow = () => setArmed(false);
    const onPointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) {
        disarmNow();
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        disarmNow();
      }
    };

    const timer = window.setTimeout(disarmNow, DISARM_MS);
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [armed]);

  return { arm, armed, disarm, ref };
}
