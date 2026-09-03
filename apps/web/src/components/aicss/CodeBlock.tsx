"use client";

import { useState } from "react";
import styles from "./CodeBlock.module.css";

export function CodeBlock({ lang, code }: { lang: string; code: string }) {
  const [copied, setCopied] = useState(false);
  const lines = code.split("\n");
  const copy = () => {
    navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };
  return (
    <div className={styles.cb}>
      <div className={styles.cbHead}>
        <span className={styles.cbFile}>
          <svg
            aria-hidden="true"
            className={styles.cbIcon}
            height="15"
            viewBox="0 0 24 24"
            width="15"
          >
            <path
              d="m8 6-6 6 6 6M16 6l6 6-6 6"
              fill="none"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="1.8"
            />
          </svg>
          <span className={styles.cbLang}>{lang}</span>
        </span>
        <button
          aria-label={copied ? "Copied" : "Copy code"}
          className={styles.cbCopy}
          onClick={copy}
        >
          {copied ? (
            <svg
              aria-hidden="true"
              fill="none"
              height="13"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
              viewBox="0 0 24 24"
              width="13"
            >
              <path d="m4.5 12.75 6 6 9-13.5" />
            </svg>
          ) : (
            <svg
              aria-hidden="true"
              fill="none"
              height="13"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="1.7"
              viewBox="0 0 24 24"
              width="13"
            >
              <rect height="11" rx="2.5" width="11" x="9" y="9" />
              <path d="M5 15a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2" />
            </svg>
          )}
          <span>{copied ? "Copied" : "Copy"}</span>
        </button>
      </div>
      <div className={styles.cbBody}>
        <div className={styles.cbLines}>
          {lines.map((line, i) => (
            <div className={styles.cbRow} key={i}>
              <span className={styles.cbLn}>{i + 1}</span>
              <code className={styles.cbCode}>{line || "\u00A0"}</code>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
