import { describe, expect, it } from "vitest";
import { isAppError, toAppError } from "./app-error";

describe("isAppError", () => {
  it.each([
    [{ code: "internal", message: "boom" }, true],
    [{ code: "internal", message: "boom", details: "x" }, true],
    [{ code: 42, message: "boom" }, false],
    [{ code: "internal" }, false],
    [{ message: "boom" }, false],
    [null, false],
    [undefined, false],
    ["boom", false],
    [42, false],
    [new Error("boom"), false],
  ])("returns %s for %o", (value, expected) => {
    expect(isAppError(value)).toBe(expected);
  });
});

describe("toAppError", () => {
  it("returns an AppError as-is", () => {
    const appError = { code: "diff-too-large" as const, message: "too big" };
    expect(toAppError(appError)).toBe(appError);
  });

  it("wraps an Error instance", () => {
    expect(toAppError(new Error("oops"))).toEqual({
      code: "internal",
      message: "oops",
    });
  });

  it("stringifies unknown values", () => {
    expect(toAppError("plain string")).toEqual({
      code: "internal",
      message: "plain string",
    });
    expect(toAppError(42)).toEqual({ code: "internal", message: "42" });
    expect(toAppError(null)).toEqual({ code: "internal", message: "null" });
  });
});
