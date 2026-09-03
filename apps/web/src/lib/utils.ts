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
