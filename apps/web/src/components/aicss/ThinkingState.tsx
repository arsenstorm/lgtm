"use client";

import styles from "./ThinkingState.module.css";

export function ThinkingState({ label = "Thinking" }: { label?: string } = {}) {
  return <span className={styles.shimmer}>{label}</span>;
}
