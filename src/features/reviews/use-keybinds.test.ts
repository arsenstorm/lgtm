import { describe, expect, it } from "vitest";
import { applyKeybindOverrides, DEFAULT_KEYBINDS } from "./use-keybinds";

describe("applyKeybindOverrides", () => {
  it("overlays valid overrides and normalizes case", () => {
    expect(applyKeybindOverrides({ "next-file": "X" })).toEqual({
      ...DEFAULT_KEYBINDS,
      "next-file": "x",
    });
  });

  it("ignores unknown ids, non-strings, and multi-character values", () => {
    expect(
      applyKeybindOverrides({
        bogus: "q",
        "prev-file": 7,
        comment: "Enter",
      })
    ).toEqual(DEFAULT_KEYBINDS);
    expect(applyKeybindOverrides(null)).toEqual(DEFAULT_KEYBINDS);
    expect(applyKeybindOverrides("[]")).toEqual(DEFAULT_KEYBINDS);
  });
});
