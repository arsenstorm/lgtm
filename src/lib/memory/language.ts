const EXTENSION_TO_LANGUAGE: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  rs: "rust",
  py: "python",
  go: "go",
  rb: "ruby",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  swift: "swift",
  c: "c",
  h: "c",
  cc: "cpp",
  cpp: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  cs: "csharp",
  php: "php",
  css: "css",
  scss: "scss",
  html: "html",
  json: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  sql: "sql",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  md: "markdown",
  markdown: "markdown",
  vue: "vue",
  svelte: "svelte",
};

/** Detects a memory-engine language identifier from a file path's extension. */
export function detectLanguage(filePath: string): string | null {
  const lastDot = filePath.lastIndexOf(".");
  if (lastDot === -1) {
    return null;
  }
  const extension = filePath.slice(lastDot + 1).toLowerCase();
  return EXTENSION_TO_LANGUAGE[extension] ?? null;
}
