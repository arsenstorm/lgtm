import { describe, expect, it } from "vitest";
import {
  lineRangeFromRows,
  pathTouchesGutter,
  rowFromEventPath,
} from "./dom-selection";

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

function row(code: HTMLElement, line: number): HTMLElement {
  const el = code.querySelector<HTMLElement>(`[data-line="${line}"]`);
  if (!el) {
    throw new Error(`fixture missing row for line ${line}`);
  }
  return el;
}

describe("lineRangeFromRows", () => {
  it("maps two addition rows to an additions range", () => {
    const code = buildCode(
      "data-additions",
      `<div data-line="12" data-line-type="change-addition">a</div>
       <div data-line="13" data-line-type="change-addition">b</div>
       <div data-line="14" data-line-type="change-addition">c</div>
       <div data-line="15" data-line-type="change-addition">d</div>`
    );

    expect(lineRangeFromRows(row(code, 12), row(code, 15))).toEqual({
      start: 12,
      end: 15,
      side: "additions",
      endSide: "additions",
    });
  });

  it("normalizes reversed row order to the same range", () => {
    const code = buildCode(
      "data-additions",
      `<div data-line="12" data-line-type="change-addition">a</div>
       <div data-line="15" data-line-type="change-addition">d</div>`
    );

    expect(lineRangeFromRows(row(code, 15), row(code, 12))).toEqual({
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

    expect(lineRangeFromRows(row(code, 4), row(code, 6))).toEqual({
      start: 4,
      end: 6,
      side: "deletions",
      endSide: "deletions",
    });
  });

  it("returns null when rows are on opposite sides of a split diff", () => {
    const deletions = buildCode(
      "data-deletions",
      `<div data-line="4" data-line-type="change-deletion">a</div>`
    );
    const additions = buildCode(
      "data-additions",
      `<div data-line="4" data-line-type="change-addition">a</div>`
    );

    expect(lineRangeFromRows(row(deletions, 4), row(additions, 4))).toBeNull();
  });

  it("returns null when a row is outside any data-code container", () => {
    const code = buildCode(
      "data-additions",
      `<div data-line="12" data-line-type="change-addition">a</div>`
    );
    const orphan = document.createElement("div");
    orphan.setAttribute("data-line", "3");
    orphan.setAttribute("data-line-type", "context");
    document.body.append(orphan);

    expect(lineRangeFromRows(row(code, 12), orphan)).toBeNull();
  });
});

describe("rowFromEventPath", () => {
  it("resolves the row from the deepest element in a composed path", () => {
    const code = buildCode(
      "data-additions",
      `<div data-line="12" data-line-type="change-addition"><span>tok</span></div>`
    );
    const span = code.querySelector("span") as HTMLElement;

    expect(rowFromEventPath([span, row(code, 12), code, document.body])).toBe(
      row(code, 12)
    );
  });

  it("returns null for a path with no diff row", () => {
    const outside = document.createElement("span");
    document.body.append(outside);

    expect(rowFromEventPath([outside, document.body, document])).toBeNull();
  });
});

describe("pathTouchesGutter", () => {
  it("detects a press on a line-number cell", () => {
    const cell = document.createElement("div");
    cell.setAttribute("data-column-number", "12");
    document.body.append(cell);

    expect(pathTouchesGutter([cell, document.body])).toBe(true);
    expect(pathTouchesGutter([document.body])).toBe(false);
  });
});
