import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("Koushi logo asset", () => {
  it("is a byte-identical mirror of the shipped application icon", () => {
    // The frontend may not import from src-tauri, so the brand mark is
    // mirrored; this keeps it from drifting into an independent logo.
    const mirror = readFileSync(new URL("./koushi-logo.svg", import.meta.url));
    const appIcon = readFileSync(new URL("../../src-tauri/icons/icon.svg", import.meta.url));
    expect(mirror.equals(appIcon)).toBe(true);
  });
});
