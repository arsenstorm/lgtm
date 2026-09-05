"use client";

import { useState } from "react";
import { CheckIcon, CodeIcon, CopyIcon } from "@/components/icons";
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
          <CodeIcon className={styles.cbIcon} height={15} width={15} />
          <span className={styles.cbLang}>{lang}</span>
        </span>
        <button
          aria-label={copied ? "Copied" : "Copy code"}
          className={styles.cbCopy}
          onClick={copy}
        >
          {copied ? (
            <CheckIcon height={13} width={13} />
          ) : (
            <CopyIcon height={13} width={13} />
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
