import type Database from "@tauri-apps/plugin-sql";

type ExecuteResult = { rowsAffected: number; lastInsertId: number };

const DEFAULT_EXECUTE_RESULT: ExecuteResult = {
  rowsAffected: 1,
  lastInsertId: 0,
};

export function createFakeDb(): {
  db: Database;
  calls: { sql: string; params: unknown[] }[];
  enqueueSelect: (rows: unknown[]) => void;
  enqueueExecute: (result: ExecuteResult) => void;
} {
  const calls: { sql: string; params: unknown[] }[] = [];
  const queue: unknown[][] = [];
  const executeQueue: ExecuteResult[] = [];

  const fake = {
    select: (sql: string, params: unknown[] = []) => {
      calls.push({ sql, params });
      return Promise.resolve(queue.shift() ?? []);
    },
    execute: (sql: string, params: unknown[] = []) => {
      calls.push({ sql, params });
      return Promise.resolve(executeQueue.shift() ?? DEFAULT_EXECUTE_RESULT);
    },
  };

  return {
    db: fake as unknown as Database,
    calls,
    enqueueSelect: (rows: unknown[]) => {
      queue.push(rows);
    },
    enqueueExecute: (result: ExecuteResult) => {
      executeQueue.push(result);
    },
  };
}
