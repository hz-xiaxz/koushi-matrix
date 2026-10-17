// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AdvisoryDirectMessageSettlement } from "../../domain/types";
import { t } from "../../i18n/messages";
import { MessageMeta } from "./MessageMeta";
import { SenderDeviceAdvisoryDialog, advisoryText } from "./SenderDeviceAdvisoryDialog";

afterEach(cleanup);

// #1494: the sender-name ⓘ and its advisory dialog. Core owns whether a
// direct chat exists and whether the draft was seeded; React only renders the
// settlement it gets back.

const NAME = "Bob Example";

function settlement(
  status: AdvisoryDirectMessageSettlement["status"],
  draft: AdvisoryDirectMessageSettlement["draft"]
): AdvisoryDirectMessageSettlement {
  return { protocolVersion: 1, publishedGeneration: 1, status, draft };
}

describe("sender device advisory badge", () => {
  const baseProps = {
    senderDisplayLabel: NAME,
    timestampMs: 1_800_000_000_000,
    isEdited: false,
    isRedacted: false,
    sendStateKind: null
  };

  it("renders only when the row binds an advisory action", () => {
    render(<MessageMeta {...baseProps} />);
    expect(
      screen.queryByRole("button", { name: t("people.unverifiedDevices.badgeLabel", { name: NAME }) })
    ).toBeNull();
  });

  it("opens the advisory without activating the row", () => {
    const onOpen = vi.fn();
    const onParentClick = vi.fn();
    render(
      <div onClick={onParentClick}>
        <MessageMeta {...baseProps} onOpenSenderDeviceAdvisory={onOpen} />
      </div>
    );
    fireEvent.click(
      screen.getByRole("button", { name: t("people.unverifiedDevices.badgeLabel", { name: NAME }) })
    );
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(onParentClick).not.toHaveBeenCalled();
  });
});

describe("sender device advisory dialog", () => {
  function renderDialog(onSendAdvisory: (text: string, create: boolean) => Promise<AdvisoryDirectMessageSettlement>) {
    const onClose = vi.fn();
    const onOpenUserInfo = vi.fn();
    render(
      <SenderDeviceAdvisoryDialog
        senderDisplayLabel={NAME}
        onClose={onClose}
        onOpenUserInfo={onOpenUserInfo}
        onSendAdvisory={onSendAdvisory}
      />
    );
    return { onClose, onOpenUserInfo };
  }

  function preview(): string | null | undefined {
    return document.querySelector(".sender-device-advisory-preview p")?.textContent;
  }

  function sendButton() {
    return screen.getByRole("button", { name: t("people.unverifiedDevices.sendAdvisory") });
  }

  it("previews the advisory in the selected language and sends that text", async () => {
    const onSend = vi.fn().mockResolvedValue(settlement("opened", "seeded"));
    const { onClose } = renderDialog(onSend);
    expect(preview()).toBe(advisoryText(NAME, "en"));

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "ja" } });
    expect(preview()).toBe(advisoryText(NAME, "ja"));
    expect(advisoryText(NAME, "ja")).not.toBe(advisoryText(NAME, "en"));

    await act(async () => fireEvent.click(sendButton()));
    expect(onSend).toHaveBeenCalledWith(advisoryText(NAME, "ja"), false);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("asks before creating a missing direct chat", async () => {
    const onSend = vi
      .fn()
      .mockResolvedValueOnce(settlement("missing", null))
      .mockResolvedValueOnce(settlement("opened", "seeded"));
    const { onClose } = renderDialog(onSend);

    await act(async () => fireEvent.click(sendButton()));
    expect(onSend).toHaveBeenLastCalledWith(advisoryText(NAME, "en"), false);
    expect(screen.getByText(t("people.unverifiedDevices.createDmBody", { name: NAME }))).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();

    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: t("people.unverifiedDevices.createDmConfirm") }))
    );
    expect(onSend).toHaveBeenLastCalledWith(advisoryText(NAME, "en"), true);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("cancelling the creation prompt creates nothing", async () => {
    const onSend = vi.fn().mockResolvedValue(settlement("missing", null));
    const { onClose } = renderDialog(onSend);

    await act(async () => fireEvent.click(sendButton()));
    fireEvent.click(screen.getByRole("button", { name: t("action.cancel") }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).not.toHaveBeenCalledWith(expect.anything(), true);
  });

  it("explains a kept draft instead of closing silently", async () => {
    const onSend = vi.fn().mockResolvedValue(settlement("opened", "kept_existing"));
    const { onClose } = renderDialog(onSend);

    await act(async () => fireEvent.click(sendButton()));
    expect(screen.getByText(t("people.unverifiedDevices.keptExistingDraft"))).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("hands off to User info", () => {
    const onSend = vi.fn();
    const { onClose, onOpenUserInfo } = renderDialog(onSend);
    fireEvent.click(screen.getByRole("button", { name: t("people.unverifiedDevices.openUserInfo") }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onOpenUserInfo).toHaveBeenCalledTimes(1);
    expect(onSend).not.toHaveBeenCalled();
  });
});
