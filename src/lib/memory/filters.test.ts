import { describe, expect, it } from "vitest";
import {
  hasSubstantiveBody,
  hasSubstantiveCode,
  isExcludedPath,
  isGenericComment,
  isImportBlock,
  isMinifiedCode,
} from "./filters";

describe("isExcludedPath", () => {
  it.each([
    ["node_modules/foo/index.js", true],
    ["src/node_modules/foo.ts", true],
    ["vendor/lib.php", true],
    ["dist/bundle.js", true],
    ["build/output.js", true],
    ["target/debug/main.rs", true],
    ["__generated__/schema.ts", true],
    ["__snapshots__/App.test.ts.snap", true],
    [".next/static/chunk.js", true],
    ["out/index.html", true],
    ["src/deep/node_modules/x.ts", true],
    ["package-lock.json", true],
    ["yarn.lock", true],
    ["bun.lock", true],
    ["bun.lockb", true],
    ["pnpm-lock.yaml", true],
    ["Cargo.lock", true],
    ["composer.lock", true],
    ["Gemfile.lock", true],
    ["go.sum", true],
    ["poetry.lock", true],
    ["uv.lock", true],
    ["src/app.min.js", true],
    ["src/App.test.ts.snap", true],
    ["src/schema.generated.ts", true],
    ["src/app.ts", false],
    ["src/components/Button.tsx", false],
    ["README.md", false],
    ["src/lib/memory/engine.ts", false],
    ["src/minifier/lib.ts", false],
  ])("isExcludedPath(%s) === %s", (path, expected) => {
    expect(isExcludedPath(path)).toBe(expected);
  });
});

describe("isMinifiedCode", () => {
  it("is false for normal code", () => {
    expect(isMinifiedCode("const a = 1;\nconst b = 2;")).toBe(false);
  });

  it("is true when a line exceeds the max reasonable length", () => {
    const longLine = `const a = "${"x".repeat(600)}";`;
    expect(isMinifiedCode(longLine)).toBe(true);
  });
});

describe("isImportBlock", () => {
  it("is true when every non-empty line is an import statement", () => {
    const code = "import { a } from './a';\n\nimport { b } from './b';";
    expect(isImportBlock(code)).toBe(true);
  });

  it("recognizes python-style imports", () => {
    expect(isImportBlock("from os import path\nimport sys")).toBe(true);
  });

  it("recognizes rust use statements", () => {
    expect(isImportBlock("use std::collections::HashMap;")).toBe(true);
  });

  it("recognizes require and #include", () => {
    expect(isImportBlock("require('fs');")).toBe(true);
    expect(isImportBlock("#include <stdio.h>")).toBe(true);
  });

  it("is false when any line is not an import", () => {
    const code = "import { a } from './a';\nconst x = a();";
    expect(isImportBlock(code)).toBe(false);
  });

  it("is false for empty code", () => {
    expect(isImportBlock("")).toBe(false);
  });
});

describe("isGenericComment", () => {
  it.each([
    "nit",
    "Nit.",
    "why",
    "why?",
    "fix this",
    "fix",
    "todo",
    "lgtm",
    "ship it",
    "+1",
    "👍",
    "remove",
    "delete this",
    "typo",
    "formatting",
    "spacing",
    "same here",
    "same",
    "this too",
    "ditto",
  ])("treats %s as generic", (body) => {
    expect(isGenericComment(body)).toBe(true);
  });

  it("is false for substantive comments", () => {
    expect(
      isGenericComment(
        "This function mutates the shared cache without a lock, which will race under concurrent requests."
      )
    ).toBe(false);
  });
});

describe("hasSubstantiveBody", () => {
  it("is true for a long, specific comment", () => {
    expect(
      hasSubstantiveBody(
        "This should use a Set instead of an array for O(1) lookups here."
      )
    ).toBe(true);
  });

  it("is false for short comments", () => {
    expect(hasSubstantiveBody("looks fine")).toBe(false);
  });

  it("is false for generic comments even if long enough", () => {
    expect(hasSubstantiveBody("nit")).toBe(false);
  });

  it("is false for comments with fewer than 4 words", () => {
    expect(hasSubstantiveBody("extraordinarily verbose")).toBe(false);
  });
});

describe("hasSubstantiveCode", () => {
  it("is true for code with at least 8 tokens", () => {
    expect(hasSubstantiveCode("const items = await fetchUsers();")).toBe(true);
  });

  it("is false for trivial code", () => {
    expect(hasSubstantiveCode("a();")).toBe(false);
  });
});
