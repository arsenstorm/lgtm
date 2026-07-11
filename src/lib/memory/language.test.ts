import { describe, expect, it } from "vitest";
import { detectLanguage } from "./language";

describe("detectLanguage", () => {
  it.each([
    ["src/app.ts", "typescript"],
    ["src/app.tsx", "typescript"],
    ["src/app.mts", "typescript"],
    ["src/app.cts", "typescript"],
    ["src/app.js", "javascript"],
    ["src/app.jsx", "javascript"],
    ["src/app.mjs", "javascript"],
    ["src/app.cjs", "javascript"],
    ["src/main.rs", "rust"],
    ["script.py", "python"],
    ["main.go", "go"],
    ["app.rb", "ruby"],
    ["Main.java", "java"],
    ["Main.kt", "kotlin"],
    ["Main.kts", "kotlin"],
    ["App.swift", "swift"],
    ["lib.c", "c"],
    ["lib.h", "c"],
    ["lib.cc", "cpp"],
    ["lib.cpp", "cpp"],
    ["lib.cxx", "cpp"],
    ["lib.hpp", "cpp"],
    ["lib.hh", "cpp"],
    ["Program.cs", "csharp"],
    ["index.php", "php"],
    ["style.css", "css"],
    ["style.scss", "scss"],
    ["index.html", "html"],
    ["data.json", "json"],
    ["config.yaml", "yaml"],
    ["config.yml", "yaml"],
    ["Cargo.toml", "toml"],
    ["query.sql", "sql"],
    ["run.sh", "shell"],
    ["run.bash", "shell"],
    ["run.zsh", "shell"],
    ["README.md", "markdown"],
    ["README.markdown", "markdown"],
    ["App.vue", "vue"],
    ["App.svelte", "svelte"],
  ])("maps %s to %s", (path, language) => {
    expect(detectLanguage(path)).toBe(language);
  });

  it("is case-insensitive on extension", () => {
    expect(detectLanguage("src/App.TS")).toBe("typescript");
  });

  it("uses the last dot segment", () => {
    expect(detectLanguage("archive.tar.gz")).toBeNull();
  });

  it("returns null for unknown extensions", () => {
    expect(detectLanguage("Dockerfile")).toBeNull();
    expect(detectLanguage("data.bin")).toBeNull();
  });

  it("returns null when there is no extension", () => {
    expect(detectLanguage("Makefile")).toBeNull();
  });
});
