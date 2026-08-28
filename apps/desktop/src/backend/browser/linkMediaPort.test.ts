/* @vitest-environment jsdom */

import { expect, test, vi } from "vitest";

import { browserLinkMediaPort } from "./linkMediaPort";

test("browser link/media operations use native web behavior", async () => {
  const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);

  await browserLinkMediaPort.openHttpUrl("https://example.com/");
  expect(windowOpen).toHaveBeenCalledWith(
    "https://example.com/",
    "_blank",
    "noopener,noreferrer"
  );
  expect(browserLinkMediaPort.mediaSourceUrl("/tmp/media.png")).toBe("/tmp/media.png");
  await expect(browserLinkMediaPort.saveMediaFile("/tmp/media.png", "media.png")).resolves.toBe(
    undefined
  );
});
