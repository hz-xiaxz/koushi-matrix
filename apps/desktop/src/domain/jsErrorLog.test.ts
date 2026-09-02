import { beforeEach, describe, expect, test } from "vitest";

import {
  getRecentJsErrors,
  installJsErrorCapture,
  jsErrorAgeBucket,
  recordJsError,
  resetJsErrors
} from "./jsErrorLog";

describe("jsErrorLog", () => {
  beforeEach(() => {
    resetJsErrors();
  });

  test("starts empty", () => {
    expect(getRecentJsErrors()).toEqual([]);
  });

  test("buckets monotonic session age at fixed boundaries", () => {
    expect(jsErrorAgeBucket(59_999)).toBe("<1m");
    expect(jsErrorAgeBucket(60_000)).toBe("1m-5m");
    expect(jsErrorAgeBucket(300_000)).toBe("5m-30m");
    expect(jsErrorAgeBucket(1_800_000)).toBe("30m+");
  });

  test("captures only an allowlisted kind and fixed channel", () => {
    recordJsError(new TypeError("boom"), "window_error");

    expect(getRecentJsErrors()).toEqual([
      expect.objectContaining({ kind: "type_error", channel: "window_error" })
    ]);
  });

  test("does not retain messages, custom names, locations, stacks, paths, URLs, or tokens", () => {
    const privateDetails = [
      "secret message body",
      "!private-room:example.invalid",
      "@private-user:example.invalid",
      "$private-event:example.invalid",
      "/Users/member/private/app.tsx",
      "https://private.example.invalid/room",
      "access_token=private-token"
    ];
    const error = new Error(privateDetails.join(" "));
    error.name = "PrivateCustomError";
    error.stack = `PrivateCustomError: ${privateDetails.join(" ")}\n at ${privateDetails[4]}:10:5`;

    recordJsError(error, "unhandled_rejection");

    const serialized = JSON.stringify(getRecentJsErrors());
    expect(getRecentJsErrors()).toEqual([
      expect.objectContaining({ kind: "error", channel: "unhandled_rejection" })
    ]);
    for (const privateDetail of privateDetails) {
      expect(serialized).not.toContain(privateDetail);
    }
    expect(serialized).not.toContain("PrivateCustomError");
  });

  test("fingerprints recurring errors without retaining their source details", () => {
    const recurring = new Error("private recurring message");
    recurring.stack = "Error: private recurring message\n at /Users/member/private/app.tsx:10:5";
    recordJsError(recurring, "window_error");
    recordJsError(recurring, "window_error");
    recordJsError(new TypeError("different private message"), "window_error");

    const captured = getRecentJsErrors();
    expect(captured[0]?.ageBucket).toBe("<1m");
    expect(captured[0]?.fingerprint).toMatch(/^f1_[0-9a-f]{8}$/);
    expect(captured[1]?.fingerprint).toBe(captured[0]?.fingerprint);
    expect(captured[2]?.fingerprint).not.toBe(captured[0]?.fingerprint);
    expect(JSON.stringify(captured)).not.toContain("private recurring message");
    expect(JSON.stringify(captured)).not.toContain("/Users/member/private/app.tsx");
  });

  test("bounds the buffer to the most recent errors", () => {
    for (let i = 0; i < 30; i += 1) {
      recordJsError(new Error(`e${i}`), "window_error");
    }
    const captured = getRecentJsErrors();
    expect(captured.length).toBe(20);
    expect(captured[captured.length - 1]).toMatchObject({
      kind: "error",
      channel: "window_error"
    });
  });

  describe("installJsErrorCapture", () => {
    function makeFakeWindow() {
      const handlers: Record<string, Set<(event: unknown) => void>> = {};
      return {
        addEventListener(type: string, handler: (event: unknown) => void) {
          (handlers[type] ??= new Set()).add(handler);
        },
        removeEventListener(type: string, handler: (event: unknown) => void) {
          handlers[type]?.delete(handler);
        },
        emit(type: string, event: unknown) {
          handlers[type]?.forEach((handler) => handler(event));
        }
      };
    }

    test("records error events through the installed listener", () => {
      const fake = makeFakeWindow();
      installJsErrorCapture(fake as unknown as Window);
      const privateFilename =
        "/Users/member/private/chunk.js?source=https://private.example.invalid&access_token=private-token";

      fake.emit("error", {
        error: new RangeError("secret message body"),
        filename: privateFilename,
        lineno: 7,
        colno: 3
      });

      expect(getRecentJsErrors()).toEqual([
        expect.objectContaining({ kind: "range_error", channel: "window_error" })
      ]);
      const serialized = JSON.stringify(getRecentJsErrors());
      expect(serialized).not.toContain("secret message body");
      expect(serialized).not.toContain(privateFilename);
      expect(serialized).not.toContain("private-token");
    });

    test("records unhandled rejection reasons", () => {
      const fake = makeFakeWindow();
      installJsErrorCapture(fake as unknown as Window);

      fake.emit("unhandledrejection", { reason: new Error("rejected") });

      expect(getRecentJsErrors()[0]).toMatchObject({
        kind: "error",
        channel: "unhandled_rejection"
      });
    });

    test("stops recording after uninstall", () => {
      const fake = makeFakeWindow();
      const stop = installJsErrorCapture(fake as unknown as Window);
      stop();

      fake.emit("error", { error: new Error("after") });

      expect(getRecentJsErrors()).toEqual([]);
    });
  });
});
