import { describe, expect, it } from "vitest";

import { toExternalHttpUrl } from "./externalLinks";

describe("externalLinks", () => {
  it("normalizes http URLs", () => {
    expect(toExternalHttpUrl("https://example.com/page")).toBe("https://example.com/page");
    expect(toExternalHttpUrl("http://example.com/path?q=1")).toBe(
      "http://example.com/path?q=1"
    );
  });

  it("rejects non-http and malformed URLs", () => {
    expect(toExternalHttpUrl("javascript:alert(1)")).toBeNull();
    expect(toExternalHttpUrl("file:///tmp/secret.txt")).toBeNull();
    expect(toExternalHttpUrl("not a URL")).toBeNull();
    expect(toExternalHttpUrl(null)).toBeNull();
  });
});
