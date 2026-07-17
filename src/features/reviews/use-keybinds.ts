import { useSyncExternalStore } from "react";
import { getSetting, setSetting } from "@/lib/db/settings";

const KEYBINDS_SETTING_KEY = "review-keybinds";

/** The single-key review shortcuts a user may rebind. ⌘K is fixed. */
export const DEFAULT_KEYBINDS = {
  "next-file": "j",
  "prev-file": "k",
  "next-comment": "n",
  "prev-comment": "p",
  comment: "c",
  "toggle-viewed": "v",
  refresh: "r",
  summary: "s",
} as const;

export type KeybindableId = keyof typeof DEFAULT_KEYBINDS;
export type Keybinds = Record<KeybindableId, string>;

export type SetKeyResult =
  | { ok: true }
  | { ok: false; conflictWith: KeybindableId };

const KEYBIND_IDS = Object.keys(DEFAULT_KEYBINDS) as KeybindableId[];

/**
 * Overlays persisted overrides onto the defaults, ignoring anything that isn't
 * a known action id mapped to a single character (corrupt or stale JSON must
 * never break the shortcuts).
 */
export function applyKeybindOverrides(overrides: unknown): Keybinds {
  const next: Keybinds = { ...DEFAULT_KEYBINDS };
  if (overrides !== null && typeof overrides === "object") {
    for (const id of KEYBIND_IDS) {
      const value = (overrides as Record<string, unknown>)[id];
      if (typeof value === "string" && value.length === 1) {
        next[id] = value.toLowerCase();
      }
    }
  }
  return next;
}

// Module-level store so the workspace shortcuts and the settings dialog stay
// in sync without prop threading.
let current: Keybinds = { ...DEFAULT_KEYBINDS };
let loadStarted = false;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) {
    listener();
  }
}

async function load(): Promise<void> {
  try {
    const raw = await getSetting(KEYBINDS_SETTING_KEY);
    if (raw) {
      current = applyKeybindOverrides(JSON.parse(raw));
      emit();
    }
  } catch {
    // Unreadable setting: stay on defaults; the next edit re-persists.
  }
}

function subscribe(listener: () => void): () => void {
  if (!loadStarted) {
    loadStarted = true;
    load();
  }
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function snapshot(): Keybinds {
  return current;
}

async function persist(): Promise<void> {
  const overrides: Partial<Record<KeybindableId, string>> = {};
  for (const id of KEYBIND_IDS) {
    if (current[id] !== DEFAULT_KEYBINDS[id]) {
      overrides[id] = current[id];
    }
  }
  await setSetting(KEYBINDS_SETTING_KEY, JSON.stringify(overrides));
}

/**
 * Rebinds one action. Keys are single characters, matched case-insensitively;
 * a key already held by another action is rejected, not swapped.
 */
export async function setKeybind(
  id: KeybindableId,
  key: string
): Promise<SetKeyResult> {
  const normalized = key.toLowerCase();
  if (normalized.length !== 1) {
    return { ok: true };
  }
  const conflictWith = KEYBIND_IDS.find(
    (other) => other !== id && current[other] === normalized
  );
  if (conflictWith) {
    return { ok: false, conflictWith };
  }
  current = { ...current, [id]: normalized };
  emit();
  await persist();
  return { ok: true };
}

export async function resetKeybinds(): Promise<void> {
  current = { ...DEFAULT_KEYBINDS };
  emit();
  await persist();
}

/** Live keybind map; identical across every subscribed component. */
export function useKeybinds(): Keybinds {
  return useSyncExternalStore(subscribe, snapshot);
}
