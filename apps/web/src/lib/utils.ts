import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** The prompt's first line stands in for a task title everywhere one shows. */
export function firstLine(prompt: string): string {
  const line = prompt.split("\n", 1)[0]?.trim();
  return line ? line : "(no prompt)";
}

/** The model-written title once the inference lane has answered, the prompt's
 * first line until then. */
export function taskTitle(task: {
  spec: { prompt: string };
  title: string | null;
}): string {
  return task.title ?? firstLine(task.spec.prompt);
}

/** A bare composer control: the only chrome is the text going from muted to
 * foreground, so focus-visible has to carry the ring alone. */
export const BARE_CONTROL =
  "flex items-center gap-1.5 rounded-sm text-muted-foreground text-sm outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50";
