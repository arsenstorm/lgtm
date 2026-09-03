"use client";

import type { ReactNode } from "react";
import styles from "./TextResponse.module.css";

export function TextResponse({ children }: { children?: ReactNode }) {
  return <div className={styles.prose}>{children}</div>;
}
