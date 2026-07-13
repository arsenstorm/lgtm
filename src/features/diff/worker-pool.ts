import { getOrCreateWorkerPoolSingleton } from "@pierre/diffs/worker";
import DiffsWorker from "@pierre/diffs/worker/worker.js?worker";

// Module-level singleton instead of the library's WorkerPoolContextProvider:
// the provider's cleanup terminates the pool during StrictMode's dev
// double-mount, leaving a dead pool behind.
export const diffWorkerPool = getOrCreateWorkerPoolSingleton({
  poolOptions: {
    workerFactory: () => new DiffsWorker(),
    poolSize: 4,
    // Large enough to keep every file of a big PR highlighted at once; the
    // default (100) can thrash when priming wide diffs.
    totalASTLRUCacheSize: 500,
  },
  highlighterOptions: {},
});
