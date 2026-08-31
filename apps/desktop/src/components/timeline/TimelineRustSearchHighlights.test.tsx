import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";

import type { TextRange } from "../../domain/types";
import { renderFormattedBody, renderPlainTextBody } from "./TimelineMessageBody";

const hiddenSpoilers = { revealed: new Set<string>(), reveal: vi.fn() };

function ranges(...pairs: Array<[number, number]>): TextRange[] {
  return pairs.map(([start_utf16, end_utf16]) => ({ start_utf16, end_utf16 }));
}

describe("Rust-owned timeline search highlights", () => {
  test("plain text marks only the UTF-16 ranges supplied by Rust", () => {
    const html = renderToStaticMarkup(
      <>{renderPlainTextBody(
        "Alpha Beta",
        [],
        undefined,
        ranges([6, 10]),
        {},
        hiddenSpoilers,
        undefined
      )}</>
    );

    expect(html).toContain("Alpha <mark>Beta</mark>");
  });

  test("a supplied range can cross sanitized formatted-text nodes", () => {
    const html = renderToStaticMarkup(
      <>{renderFormattedBody(
        {
          html: "<strong>Alpha</strong> Beta",
          plain_text: "Alpha Beta",
          code_blocks: []
        },
        [],
        true,
        vi.fn(),
        ranges([3, 8]),
        hiddenSpoilers,
        undefined
      )}</>
    );

    expect(html.match(/<mark>/g)).toHaveLength(2);
    expect(html).toContain("<strong>Alp<mark>ha</mark></strong>");
    expect(html).toContain("<mark> Be</mark>ta");
  });

  test("no Rust range means no inline highlight even when text contains a query-like word", () => {
    const html = renderToStaticMarkup(
      <>{renderPlainTextBody(
        "Alpha Beta",
        [],
        undefined,
        [],
        {},
        hiddenSpoilers,
        undefined
      )}</>
    );

    expect(html).not.toContain("<mark>");
  });
});
