// @vitest-environment jsdom

import { afterEach, expect, test, vi } from "vitest";

import {
  INITIAL_TIMELINE_DIAGNOSTICS,
  qaRenderedDomDiagnostics,
  qaSecurityDiagnostics,
  timelineDiagnosticsEqual,
  timelineDiagnosticsLogMessage
} from "./qaDiagnostics";

const expectedInitial = {
  visibleItems: 0,
  downloadedItems: 0,
  backfill: "unknown",
  avatarMxcItems: 0,
  avatarReadyItems: 0,
  avatarPendingItems: 0,
  avatarFailedItems: 0,
  avatarMissingItems: 0,
  avatarRenderedImages: 0,
  avatarBrokenImages: 0
};

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

test("projects QA diagnostics without lifecycle ownership", () => {
  expect(INITIAL_TIMELINE_DIAGNOSTICS).toEqual(expectedInitial);

  document.body.innerHTML = '<div id="root"><div>fixture</div></div>';
  Object.defineProperty(document.body, "innerText", {
    configurable: true,
    get: () => "fixture text"
  });
  expect(qaRenderedDomDiagnostics()).toEqual({
    screen: "unknown",
    rootChildren: 1,
    bodyTextLength: 12
  });
  document.querySelector("#root")!.innerHTML = '<div data-testid="timeline-view"></div>';
  expect(qaRenderedDomDiagnostics().screen).toBe("timeline");
  document.body.insertAdjacentHTML("afterbegin", '<div data-testid="recovery-panel"></div>');
  expect(qaRenderedDomDiagnostics().screen).toBe("recovery");
  document.body.insertAdjacentHTML("afterbegin", '<div data-testid="auth-screen"></div>');
  expect(qaRenderedDomDiagnostics().screen).toBe("auth");
  document.body.insertAdjacentHTML("afterbegin", '<div data-testid="boot-error"></div>');
  expect(qaRenderedDomDiagnostics().screen).toBe("boot_error");
  document.body.innerHTML = '<div id="root"></div>';
  expect(qaRenderedDomDiagnostics()).toEqual({ screen: "empty", rootChildren: 0, bodyTextLength: 12 });

  Object.defineProperty(window, "isSecureContext", { configurable: true, value: true });
  const valid = document.createElement("img");
  valid.src = "https://cdn.example.invalid/avatar.png";
  Object.defineProperty(valid, "complete", { configurable: true, value: true });
  Object.defineProperty(valid, "naturalWidth", { configurable: true, value: 20 });
  const invalid = document.createElement("img");
  invalid.src = "not a URL";
  Object.defineProperty(invalid, "complete", { configurable: true, value: false });
  document.body.innerHTML = '<div id="root"></div>';
  const validAvatar = document.createElement("div");
  validAvatar.className = "avatar";
  validAvatar.append(valid);
  const invalidAvatar = document.createElement("div");
  invalidAvatar.className = "room-avatar";
  invalidAvatar.append(invalid);
  document.querySelector("#root")!.append(validAvatar, invalidAvatar);
  expect(qaSecurityDiagnostics()).toEqual({
    secureContext: true,
    locationProtocol: "http:",
    locationOrigin: "http://localhost:3000",
    avatarImageSchemes: { https: 1, http: 1 },
    avatarBrokenImages: 1
  });
  vi.spyOn(document, "querySelectorAll").mockReturnValue([
    valid,
    { currentSrc: "http://[", src: "", complete: true, naturalWidth: 20 } as HTMLImageElement
  ] as unknown as NodeListOf<HTMLImageElement>);
  expect(qaSecurityDiagnostics().avatarImageSchemes).toEqual({ https: 1, invalid: 1 });

  const fields = Object.keys(expectedInitial) as (keyof typeof expectedInitial)[];
  for (const field of fields) {
    const changed = { ...INITIAL_TIMELINE_DIAGNOSTICS, [field]: field === "backfill" ? "ready" : 1 };
    expect(timelineDiagnosticsEqual(INITIAL_TIMELINE_DIAGNOSTICS, changed)).toBe(false);
  }
  expect(timelineDiagnosticsEqual(INITIAL_TIMELINE_DIAGNOSTICS, { ...INITIAL_TIMELINE_DIAGNOSTICS })).toBe(true);
  expect(timelineDiagnosticsLogMessage({
    visibleItems: 1,
    downloadedItems: 2,
    backfill: "ready",
    avatarMxcItems: 3,
    avatarReadyItems: 4,
    avatarPendingItems: 5,
    avatarFailedItems: 6,
    avatarMissingItems: 7,
    avatarRenderedImages: 8,
    avatarBrokenImages: 9
  })).toBe("items visible=1 downloaded=2 backfill=ready avatars mxc=3 ready=4 pending=5 failed=6 missing=7 rendered=8 broken=9");
});
