import Database from "@tauri-apps/plugin-sql";
import type { AppError } from "../errors/app-error";

let dbPromise: Promise<Database> | null = null;

export function getDb(): Promise<Database> {
  if (!dbPromise) {
    dbPromise = Database.load("sqlite:lgtm.db").catch((e: unknown) => {
      dbPromise = null;
      throw {
        code: "database-failure",
        message: "Could not open the local database",
        details: String(e),
      } satisfies AppError;
    });
  }
  return dbPromise;
}

export function setDbForTesting(db: Database | null): void {
  dbPromise = db ? Promise.resolve(db) : null;
}
