"use client";

import type { ReactNode } from "react";
import styles from "./DataTable.module.css";

export interface DataTableRow {
  cells: ReactNode[];
  id: string;
}

export function DataTable({
  columns,
  rows,
  selected,
  onSelect,
}: {
  columns: string[];
  rows: DataTableRow[];
  /** Row id currently chosen; rows become selectable only with `onSelect`. */
  selected?: string;
  onSelect?: (id: string) => void;
}) {
  return (
    <div className={styles.tbl}>
      <div className={styles.tblHead}>
        {columns.map((h) => (
          <div className={styles.tblCell} key={h}>
            {h}
          </div>
        ))}
      </div>
      <div className={styles.tblBody}>
        {rows.map((r) => {
          const cells = r.cells.map((cell, i) => (
            // Cells are positional, so the column name is the only stable key.
            <div className={styles.tblCell} key={columns[i] ?? i}>
              <span className={styles.tblCellText}>{cell}</span>
            </div>
          ));
          if (!onSelect) {
            return (
              <div className={styles.tblRow} key={r.id}>
                {cells}
              </div>
            );
          }
          return (
            <button
              aria-pressed={selected === r.id}
              className={
                styles.tblRow +
                " " +
                styles.tblRowBtn +
                (selected === r.id ? " " + styles.tblRowOn : "")
              }
              key={r.id}
              onClick={() => onSelect(r.id)}
              type="button"
            >
              {cells}
            </button>
          );
        })}
      </div>
    </div>
  );
}
