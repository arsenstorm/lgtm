import type { FileDiffOptions, ThemeTypes } from "@pierre/diffs";
import { parsePatchFiles } from "@pierre/diffs";
import { FileDiff } from "@pierre/diffs/react";
import { useMemo, useSyncExternalStore } from "react";

// Unified over split: a review column inside the sidebar layout is too narrow
// to read two gutters side by side. `scroll` keeps long lines in their own
// column so the +/- alignment survives, which is the point of reading a diff.
const DIFF_OPTIONS: FileDiffOptions<undefined> = {
  diffStyle: "unified",
  hunkSeparators: "line-info",
  lineDiffType: "word-alt",
  overflow: "scroll",
  stickyHeader: true,
};

function subscribeToTheme(onChange: () => void) {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, {
    attributeFilter: ["class"],
    attributes: true,
  });
  return () => observer.disconnect();
}

// The library pins `color-scheme: light dark` on its own shadow host, so its
// `light-dark()` colors resolve against the OS rather than the theme class the
// app writes to <html>. Feeding it an explicit themeType is what keeps the
// diff from staying dark on a light page.
function useAppThemeType(): ThemeTypes {
  return useSyncExternalStore(
    subscribeToTheme,
    () =>
      document.documentElement.classList.contains("dark") ? "dark" : "light",
    () => "system"
  );
}

export function DiffView({
  diff,
  cacheKey,
}: {
  diff: string;
  cacheKey: string;
}) {
  const themeType = useAppThemeType();
  const options = useMemo(() => ({ ...DIFF_OPTIONS, themeType }), [themeType]);
  const files = useMemo(
    () => parsePatchFiles(diff, cacheKey).flatMap((patch) => patch.files),
    [diff, cacheKey]
  );

  if (files.length === 0) {
    return (
      <div className="rounded-lg border border-dashed p-4">
        <p className="font-medium text-sm">This patch produced no file diffs</p>
        <p className="mt-1 max-w-[54ch] text-pretty text-muted-foreground text-sm">
          The raw patch is below, exactly as the runner reported it.
        </p>
        <pre className="mt-3 overflow-x-auto text-xs leading-relaxed">
          {diff}
        </pre>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {files.map((file) => (
        <div
          className="overflow-hidden rounded-lg border border-border"
          key={file.cacheKey ?? file.name}
        >
          <FileDiff fileDiff={file} options={options} />
        </div>
      ))}
    </div>
  );
}
