// @vitest-environment jsdom

import { act, cleanup, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { ComposerDocument } from "../domain/types";
import { ActiveLocaleContext, activeLocaleKey } from "../i18n/activeLocaleContext";
import { setActiveLocaleProfile, type Locale } from "../i18n/messages";
import { Composer, ThreadComposer } from "./composer";

// Stable props: a fresh callback identity would re-render the memoised
// composers on its own and hide the stale-locale defect (#1557).
const noop = () => undefined;
const asyncNoop = async () => undefined;
const resolveNoop = async () => "noop" as const;
const draft: ComposerDocument = {
  version: 2,
  inlines: [
    { kind: "text", text: "draft " },
    {
      kind: "mention",
      target: { kind: "user", user_id: "@member-one:example.invalid", display_label: "Member 1" },
      display_label: "Member 1"
    },
    { kind: "text", text: " kept" }
  ]
};
const composerMode = { kind: "plain" } as const;

let switchLocale: (locale: Locale) => void = () => undefined;

function LocaleHarness({ thread = false }: { thread?: boolean }) {
  const [locale, setLocale] = useState<Locale>("en");
  switchLocale = setLocale;
  // Mirrors App: the module-level catalog is set during render and the
  // context carries the same profile.
  setActiveLocaleProfile(locale);
  return (
    <ActiveLocaleContext.Provider value={activeLocaleKey(locale, "none")}>
      {thread ? (
        <ThreadComposer
          canEdit
          document={draft}
          draftKey="!lab:$root"
          isSending={false}
          resolveComposerKeyAction={resolveNoop}
          onAttachFiles={asyncNoop}
          onDocumentChange={noop}
          onSend={noop}
        />
      ) : (
        <Composer
          composerMode={composerMode}
          isSending={false}
          roomName="Lab"
          document={draft}
          draftKey="!lab"
          onAttachFiles={asyncNoop}
          onCancelReply={noop}
          onSend={noop}
          onDocumentChange={noop}
        />
      )}
    </ActiveLocaleContext.Provider>
  );
}

afterEach(() => {
  cleanup();
  setActiveLocaleProfile("en");
});

describe("composer locale switch (#1557)", () => {
  it("re-renders the main composer chrome in the new catalog without remounting the draft", () => {
    const { container } = render(<LocaleHarness />);
    const editor = screen.getByRole("textbox", { name: "Message composer" }) as HTMLDivElement;
    expect(editor.dataset.placeholder).toBe("Message Lab");
    expect(container.querySelector(".composer-math-toggle span")?.textContent).toBe("Math");
    const mention = editor.querySelector("[data-composer-mention]");
    expect(mention?.getAttribute("aria-label")).toBe("Mention: Member 1");

    act(() => switchLocale("ja"));

    const editorAfter = screen.getByRole("textbox", { name: "メッセージ入力欄" });
    expect(editorAfter).toBe(editor);
    expect(editor.textContent).toBe("draft @Member 1 kept");
    expect(editor.querySelector("[data-composer-mention]")).toBe(mention);
    expect(editor.dataset.placeholder).toBe("Labにメッセージ");
    expect(container.querySelector(".composer-math-toggle span")?.textContent).toBe("数式");
    expect(screen.getByRole("button", { name: "太字" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "送信" })).toBeTruthy();
    expect(mention?.getAttribute("aria-label")).toBe("メンション: Member 1");

    act(() => switchLocale("ko"));

    expect(screen.getByRole("textbox", { name: "메시지 입력창" })).toBe(editor);
    expect(editor.dataset.placeholder).toBe("Lab에 메시지 보내기");
    expect(container.querySelector(".composer-math-toggle span")?.textContent).toBe("수식");
  });

  it("re-renders the thread composer chrome in the new catalog", () => {
    render(<LocaleHarness thread />);
    const editor = screen.getByRole("textbox", { name: "Thread composer" });

    act(() => switchLocale("ja"));

    expect(screen.queryByRole("textbox", { name: "Thread composer" })).toBeNull();
    expect(screen.getAllByRole("textbox")).toContain(editor);
    expect(editor.getAttribute("aria-label")).not.toBe("Thread composer");
    expect(editor.textContent).toBe("draft @Member 1 kept");
  });
});
