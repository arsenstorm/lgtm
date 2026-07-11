/**
 * Conservative language-agnostic lexical normaliser.
 *
 * Goals: whitespace/formatting insensitivity, literal insensitivity (strings
 * and numbers become placeholders), rename insensitivity (identifiers are
 * alpha-renamed by first occurrence), while preserving keywords, operators
 * and likely API identifiers (member accesses and call targets).
 *
 * ponytail: a hand-rolled scanner, not a per-language parser. Ceiling: no
 * template-literal expression parsing, no heredocs. Swap in a Tree-sitter
 * backed CodeNormalizer implementation if precision proves insufficient.
 */

export type NormalizedCode = {
  /** Alpha-renamed token stream. */
  tokens: string[];
  /** Preserved likely-API identifiers (member names, call targets). */
  identifiers: string[];
  /** One structural shape string per non-empty line. */
  shape: string[];
  lineCount: number;
};

export type CodeNormalizer = {
  normalize(code: string): NormalizedCode;
};

const KEYWORDS = new Set([
  // shared / js / ts
  "abstract",
  "any",
  "as",
  "async",
  "await",
  "boolean",
  "break",
  "case",
  "catch",
  "class",
  "const",
  "continue",
  "default",
  "delete",
  "do",
  "else",
  "enum",
  "export",
  "extends",
  "false",
  "finally",
  "for",
  "from",
  "function",
  "get",
  "if",
  "implements",
  "import",
  "in",
  "instanceof",
  "interface",
  "let",
  "never",
  "new",
  "null",
  "number",
  "object",
  "of",
  "private",
  "protected",
  "public",
  "readonly",
  "return",
  "set",
  "static",
  "string",
  "super",
  "switch",
  "this",
  "throw",
  "true",
  "try",
  "type",
  "typeof",
  "undefined",
  "unknown",
  "var",
  "void",
  "while",
  "yield",
  // rust
  "dyn",
  "fn",
  "impl",
  "loop",
  "match",
  "mod",
  "move",
  "mut",
  "pub",
  "ref",
  "struct",
  "trait",
  "unsafe",
  "use",
  "where",
  // python
  "and",
  "def",
  "elif",
  "except",
  "is",
  "lambda",
  "none",
  "not",
  "or",
  "pass",
  "raise",
  "self",
  "with",
  // go / java / misc
  "chan",
  "defer",
  "func",
  "go",
  "int",
  "nil",
  "package",
  "range",
  "select",
  "throws",
  "final",
  "synchronized",
  "bool",
  "float",
  "str",
]);

const MULTI_CHAR_OPERATORS = [
  ">>>=",
  "===",
  "!==",
  "**=",
  "...",
  "<<=",
  ">>=",
  ">>>",
  "??=",
  "&&=",
  "||=",
  "?.",
  "??",
  "=>",
  "->",
  "==",
  "!=",
  "<=",
  ">=",
  "&&",
  "||",
  "++",
  "--",
  "+=",
  "-=",
  "*=",
  "/=",
  "%=",
  "&=",
  "|=",
  "^=",
  "**",
  "<<",
  ">>",
  "::",
];

const IDENTIFIER_START = /[A-Za-z_$]/;
const IDENTIFIER_PART = /[A-Za-z0-9_$]/;
const DIGIT = /[0-9]/;
const NUMBER_PART = /[0-9a-fA-F_xXoObB.]/;

type RawToken = {
  kind: "identifier" | "keyword" | "string" | "number" | "operator";
  text: string;
  /** identifier only: looks like an API name (after `.` or before `(`). */
  preserved?: boolean;
};

/** Returns the index after the closing quote (handles backslash escapes). */
function skipString(line: string, start: number, quote: string): number {
  let i = start;
  while (i < line.length) {
    if (line[i] === "\\") {
      i += 2;
      continue;
    }
    if (line[i] === quote) {
      return i + 1;
    }
    i++;
  }
  return i;
}

function skipNumber(line: string, start: number): number {
  let i = start;
  while (i < line.length && NUMBER_PART.test(line[i])) {
    i++;
  }
  return i;
}

/** True when a `#` or `//` at index i starts a line comment. */
function startsLineComment(line: string, i: number): boolean {
  if (line[i] === "/" && line[i + 1] === "/") {
    return true;
  }
  return (
    line[i] === "#" && (i === 0 || line[i - 1] === " " || line[i - 1] === "\t")
  );
}

function scanIdentifier(
  line: string,
  start: number,
  tokens: RawToken[]
): number {
  let i = start;
  while (i < line.length && IDENTIFIER_PART.test(line[i])) {
    i++;
  }
  const text = line.slice(start, i);
  if (KEYWORDS.has(text.toLowerCase())) {
    tokens.push({ kind: "keyword", text: text.toLowerCase() });
    return i;
  }
  const prev = tokens.at(-1);
  const afterDot =
    prev?.kind === "operator" && (prev.text === "." || prev.text === "?.");
  let j = i;
  while (j < line.length && (line[j] === " " || line[j] === "\t")) {
    j++;
  }
  const beforeCall = line[j] === "(";
  tokens.push({ kind: "identifier", text, preserved: afterDot || beforeCall });
  return i;
}

/** Scans one token at i; returns the next index, or null to stop the line. */
function scanToken(line: string, i: number, tokens: RawToken[]): number | null {
  const ch = line[i];

  if (startsLineComment(line, i)) {
    return null;
  }

  if (ch === '"' || ch === "'" || ch === "`") {
    tokens.push({ kind: "string", text: "STR" });
    return skipString(line, i + 1, ch);
  }

  if (DIGIT.test(ch) || (ch === "." && DIGIT.test(line[i + 1] ?? ""))) {
    tokens.push({ kind: "number", text: "NUM" });
    return skipNumber(line, i + 1);
  }

  if (IDENTIFIER_START.test(ch)) {
    return scanIdentifier(line, i, tokens);
  }

  const operator = MULTI_CHAR_OPERATORS.find((op) => line.startsWith(op, i));
  if (operator) {
    tokens.push({ kind: "operator", text: operator });
    return i + operator.length;
  }

  tokens.push({ kind: "operator", text: ch });
  return i + 1;
}

function scanLine(
  line: string,
  state: { inBlockComment: boolean }
): RawToken[] {
  const tokens: RawToken[] = [];
  let i = 0;

  while (i < line.length) {
    const ch = line[i];

    if (state.inBlockComment) {
      const close = line.indexOf("*/", i);
      if (close === -1) {
        return tokens;
      }
      state.inBlockComment = false;
      i = close + 2;
      continue;
    }

    if (ch === " " || ch === "\t" || ch === "\r") {
      i++;
      continue;
    }

    if (ch === "/" && line[i + 1] === "*") {
      state.inBlockComment = true;
      i += 2;
      continue;
    }

    const next = scanToken(line, i, tokens);
    if (next === null) {
      break;
    }
    i = next;
  }

  return tokens;
}

type NormalizeAccumulator = {
  aliases: Map<string, string>;
  tokens: string[];
  identifiers: Set<string>;
};

function accumulateToken(token: RawToken, acc: NormalizeAccumulator): string {
  if (token.kind === "identifier") {
    if (token.preserved) {
      acc.tokens.push(token.text);
      acc.identifiers.add(token.text);
    } else {
      let alias = acc.aliases.get(token.text);
      if (!alias) {
        alias = `id${acc.aliases.size}`;
        acc.aliases.set(token.text, alias);
      }
      acc.tokens.push(alias);
    }
    return "I";
  }
  if (token.kind === "string" || token.kind === "number") {
    acc.tokens.push(token.text);
    return "L";
  }
  acc.tokens.push(token.text);
  return token.text;
}

export const lexicalNormalizer: CodeNormalizer = {
  normalize(code: string): NormalizedCode {
    const state = { inBlockComment: false };
    const acc: NormalizeAccumulator = {
      aliases: new Map(),
      tokens: [],
      identifiers: new Set(),
    };
    const shape: string[] = [];

    for (const line of code.split("\n")) {
      const raw = scanLine(line, state);
      if (raw.length === 0) {
        continue;
      }
      shape.push(raw.map((token) => accumulateToken(token, acc)).join(" "));
    }

    return {
      tokens: acc.tokens,
      identifiers: [...acc.identifiers],
      shape,
      lineCount: shape.length,
    };
  },
};
