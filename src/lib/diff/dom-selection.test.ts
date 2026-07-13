import { describe, expect, it } from "vitest";
import { lineRangeFromNodes } from "./dom-selection";

function buildCode(attrs: string, rows: string): HTMLElement {
  const code = document.createElement("code");
  code.setAttribute("data-code", "");
  for (const attr of attrs.split(" ").filter(Boolean)) {
    code.setAttribute(attr, "");
  }
  code.innerHTML = rows;
  document.body.append(code);
  return code;
}

function row(code: HTMLElement, line: number): Node {
  const el = code.querySelector<HTMLElement>(`[data-line="${line}"]`);
  if (!el?.firstChild) {
    throw new Error(`fixture missing text node for line ${line}`);
  }
  return el.firstChild;
}

describe("lineRangeFromNodes", () => {
  it("maps two text nodes inside addition rows to an additions range", () => {
    const code = buildCode(
      "data-additions",
      `<div data-line="12" data-line-type="change-addition">a</div>
       <div data-line="13" data-line-type="change-addition">b</div>
       <div data-line="14" data-line-type="change-addition">c</div>
       <div data-line="15" data-line-type="change-addition">d</div>`
    );

    const result = lineRangeFromNodes(row(code, 12), row(code, 15));

    expect(result).toEqual({
      start: 12,
      end: 15,
      side: "additions",
      endSide: "additions",
    });
  });

  it("normalizes reversed argument order to the same range", () => {
    const code = buildCode(
      "data-additions",
      `<div data-line="12" data-line-type="change-addition">a</div>
       <div data-line="15" data-line-type="change-addition">d</div>`
    );

    const result = lineRangeFromNodes(row(code, 15), row(code, 12));

    expect(result).toEqual({
      start: 12,
      end: 15,
      side: "additions",
      endSide: "additions",
    });
  });

  it("falls back to the column's side for context rows in the deletions column", () => {
    const code = buildCode(
      "data-deletions",
      `<div data-line="4" data-line-type="context">a</div>
       <div data-line="6" data-line-type="context">b</div>`
    );

    const result = lineRangeFromNodes(row(code, 4), row(code, 6));

    expect(result).toEqual({
      start: 4,
      end: 6,
      side: "deletions",
      endSide: "deletions",
    });
  });

  it("returns null when endpoints are on opposite sides of a split diff", () => {
    const deletions = buildCode(
      "data-deletions",
      `<div data-line="4" data-line-type="change-deletion">a</div>`
    );
    const additions = buildCode(
      "data-additions",
      `<div data-line="4" data-line-type="change-addition">a</div>`
    );

    const result = lineRangeFromNodes(row(deletions, 4), row(additions, 4));

    expect(result).toBeNull();
  });

  it("returns null when an endpoint is outside any diff line row", () => {
    const code = buildCode(
      "data-additions",
      `<div data-line="12" data-line-type="change-addition">a</div>`
    );
    const outside = document.createElement("span");
    outside.textContent = "not a diff row";
    document.body.append(outside);

    const result = lineRangeFromNodes(
      row(code, 12),
      outside.firstChild as Node
    );

    expect(result).toBeNull();
  });
});
