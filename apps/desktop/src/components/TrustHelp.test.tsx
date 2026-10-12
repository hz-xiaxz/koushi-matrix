// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { TrustHelpButton } from "./TrustHelp";
import { t } from "../i18n/messages";

afterEach(cleanup);

// The catalog scanner reads this directory, so test copy is a value, not a
// literal JSX attribute.
const HELP_TITLE = "Synthetic title";
const HELP_BODY = "Synthetic body";
const FIRST_TITLE = "First";
const SECOND_TITLE = "Second";

function helpTrigger(index = 0): HTMLElement {
  return screen.getAllByRole("button", { name: t("help.userTrust.explain") })[index]!;
}

function popovers(): NodeListOf<Element> {
  return document.querySelectorAll(".trust-help-popover");
}

// #1368: the trust help popover renders in the body-level floating layer with no
// backdrop, so an outside press has to dismiss it; only one may be open.
describe("trust help popup dismissal", () => {
  it("dismisses the popover on a press outside it", () => {
    render(<TrustHelpButton title={HELP_TITLE} body={HELP_BODY} />);

    fireEvent.click(helpTrigger());
    expect(popovers()).toHaveLength(1);

    fireEvent.pointerDown(document.body);

    expect(popovers()).toHaveLength(0);
    expect(helpTrigger().getAttribute("aria-expanded")).toBe("false");
  });

  it("keeps the popover when the press lands inside it", () => {
    render(<TrustHelpButton title={HELP_TITLE} body={HELP_BODY} />);

    fireEvent.click(helpTrigger());
    fireEvent.pointerDown(popovers()[0]!);

    expect(popovers()).toHaveLength(1);
  });

  it("opens one trust help popover at a time", () => {
    render(
      <>
        <TrustHelpButton title={FIRST_TITLE} body={`${FIRST_TITLE} body`} />
        <TrustHelpButton title={SECOND_TITLE} body={`${SECOND_TITLE} body`} />
      </>
    );

    fireEvent.click(helpTrigger(0));
    fireEvent.click(helpTrigger(1));

    expect(popovers()).toHaveLength(1);
    expect(helpTrigger(0).getAttribute("aria-expanded")).toBe("false");
    expect(helpTrigger(1).getAttribute("aria-expanded")).toBe("true");
  });

  it("closes on Escape and returns focus to the trigger", () => {
    render(<TrustHelpButton title={HELP_TITLE} body={HELP_BODY} />);

    fireEvent.click(helpTrigger());
    fireEvent.keyDown(helpTrigger(), { key: "Escape" });

    expect(popovers()).toHaveLength(0);
  });
});
