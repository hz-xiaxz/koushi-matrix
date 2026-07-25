// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { StagedUploadItem } from "../domain/types";
import { CreateEntityDialog, UploadStagingDialog } from "./dialogs";

afterEach(cleanup);

function stagedImage(
  caption: string,
  preparation: StagedUploadItem["preparation"],
  stagedId = "staged-1",
  filename = "synthetic.png"
): StagedUploadItem {
  return {
    staged_id: stagedId,
    room_id: "!synthetic:example.invalid",
    position: 0,
    filename,
    mime_type: "image/png",
    byte_count: 128,
    kind: { kind: "image", width: 16, height: 16 },
    caption: caption
      ? {
          plain_body: caption,
          formatted_body: null,
          mentions: { targets: [] }
        }
      : null,
    compression_choice: { kind: "original" },
    preparation
  };
}

function dialog(items: StagedUploadItem[], onUpdateCaption = vi.fn()) {
  return (
    <UploadStagingDialog
      items={items}
      onClear={vi.fn()}
      onUpdateCaption={onUpdateCaption}
      onSelectOutput={vi.fn()}
      onSendAttachments={vi.fn()}
      onRetryPreparation={vi.fn()}
      onUseOriginal={vi.fn()}
      loadPreview={vi.fn(async () => [])}
    />
  );
}

describe("UploadStagingDialog", () => {
  it("preserves active Japanese composition across stale preparation snapshots", () => {
    const onUpdateCaption = vi.fn();
    const { rerender } = render(
      dialog([stagedImage("before", { kind: "preparing" })], onUpdateCaption)
    );
    const caption = screen.getByRole("textbox", {
      name: "Caption for synthetic.png"
    }) as HTMLInputElement;

    fireEvent.compositionStart(caption);
    fireEvent.change(caption, { target: { value: "日本語変換中" } });
    caption.setSelectionRange(3, 5);
    rerender(
      dialog(
        [
          stagedImage("before", {
            kind: "ready",
            variants: [],
            selected: { resize: "original", format: "keep" },
      pending: null,
      generation: 0
          })
        ],
        onUpdateCaption
      )
    );

    expect(caption.value).toBe("日本語変換中");
    expect([caption.selectionStart, caption.selectionEnd]).toEqual([3, 5]);
    expect(onUpdateCaption).toHaveBeenCalledWith("staged-1", "日本語変換中");
  });

  it("preserves an ordinary dirty caption until Rust acknowledges it", () => {
    const { rerender } = render(dialog([stagedImage("before", { kind: "preparing" })]));
    const caption = screen.getByRole("textbox", {
      name: "Caption for synthetic.png"
    }) as HTMLInputElement;

    fireEvent.change(caption, { target: { value: "local caption" } });
    rerender(dialog([stagedImage("before", { kind: "preparing" })]));

    expect(caption.value).toBe("local caption");
  });

  it("isolates composition ownership by staged upload identity", () => {
    const { rerender } = render(
      dialog([
        stagedImage("first", { kind: "preparing" }, "staged-a", "first.png"),
        stagedImage("second", { kind: "preparing" }, "staged-b", "second.png")
      ])
    );
    const first = screen.getByRole("textbox", {
      name: "Caption for first.png"
    }) as HTMLInputElement;
    const second = screen.getByRole("textbox", {
      name: "Caption for second.png"
    }) as HTMLInputElement;

    fireEvent.compositionStart(first);
    fireEvent.change(first, { target: { value: "一つ目を変換中" } });
    rerender(
      dialog([
        stagedImage("first", { kind: "preparing" }, "staged-a", "first.png"),
        stagedImage("second from Rust", { kind: "preparing" }, "staged-b", "second.png")
      ])
    );

    expect(first.value).toBe("一つ目を変換中");
    expect(second.value).toBe("second from Rust");
  });
});

describe("dialog IME submit handling", () => {
  it("does not create a room for candidate-confirmation Enter", () => {
    const onSubmit = vi.fn();
    const { container } = render(
      <CreateEntityDialog
        kind="room"
        isBusy={false}
        value="合成ルーム"
        onCancel={vi.fn()}
        onSubmit={onSubmit}
        onValueChange={vi.fn()}
      />
    );
    const name = screen.getByRole("textbox", { name: "Room name" });

    fireEvent.compositionStart(name);
    fireEvent.keyDown(name, {
      key: "Enter",
      code: "Enter",
      keyCode: 229,
      isComposing: true
    });
    fireEvent.submit(container.querySelector("form")!);

    expect(onSubmit).not.toHaveBeenCalled();
  });
});

// --- #305: compact resize/format controls ---

function preparedVariant(
  resize: "original" | "half" | "quarter" | "eighth",
  formatChoice: "keep" | "png" | "jpeg" | "webp",
  overrides: Partial<{ byte_count: number; width: number; height: number; savings_percent: number }> = {}
) {
  return {
    variant_id: `${resize}-${formatChoice}`,
    resize,
    format_choice: formatChoice,
    filename: `synthetic.${formatChoice === "keep" ? "png" : formatChoice}`,
    mime_type: formatChoice === "keep" ? "image/png" : `image/${formatChoice}`,
    byte_count: overrides.byte_count ?? 128,
    width: overrides.width ?? 1284,
    height: overrides.height ?? 918,
    format: (formatChoice === "keep" ? "original" : formatChoice) as
      | "original"
      | "png"
      | "jpeg"
      | "webp",
    savings_percent: overrides.savings_percent ?? 0,
    metadata_stripped: false,
    thumbnail_refreshed: false
  };
}

describe("UploadStagingDialog resize and format controls", () => {
  it("offers the four scales and four formats as compact radiogroups", () => {
    render(
      dialog([
        stagedImage("", {
          kind: "ready",
          variants: [preparedVariant("original", "keep")],
          selected: { resize: "original", format: "keep" },
          pending: null,
          generation: 0
        })
      ])
    );

    const resize = screen.getByRole("radiogroup", { name: "Resize" });
    expect(
      ["Original", "1/2", "1/4", "1/8"].map(
        (label) => within(resize).getByRole("radio", { name: label }).getAttribute("aria-checked")
      )
    ).toEqual(["true", "false", "false", "false"]);

    const format = screen.getByRole("radiogroup", { name: "Format" });
    expect(
      ["Keep", "WebP", "JPEG", "PNG"].map(
        (label) => within(format).getByRole("radio", { name: label }).getAttribute("aria-checked")
      )
    ).toEqual(["true", "false", "false", "false"]);

    // No large per-variant cards and no MIME strings survive.
    expect(document.querySelector(".upload-variant-button")).toBeNull();
    expect(screen.queryByText(/image\/png/)).toBeNull();
  });

  it("dispatches the chosen pair without changing the rendered selection itself", () => {
    const onSelectOutput = vi.fn();
    render(
      <UploadStagingDialog
        items={[
          stagedImage("", {
            kind: "ready",
            variants: [preparedVariant("original", "keep")],
            selected: { resize: "original", format: "keep" },
            pending: null,
            generation: 0
          })
        ]}
        onClear={vi.fn()}
        onUpdateCaption={vi.fn()}
        onSelectOutput={onSelectOutput}
        onRetryPreparation={vi.fn()}
        onUseOriginal={vi.fn()}
        onSendAttachments={vi.fn()}
        loadPreview={vi.fn(async () => [])}
      />
    );

    fireEvent.click(screen.getByRole("radio", { name: "1/2" }));
    expect(onSelectOutput).toHaveBeenCalledWith("staged-1", {
      resize: "half",
      format: "keep"
    });
    fireEvent.click(screen.getByRole("radio", { name: "JPEG" }));
    expect(onSelectOutput).toHaveBeenLastCalledWith("staged-1", {
      resize: "original",
      format: "jpeg"
    });
    // Rust owns the selection: the pressed state still reflects the snapshot.
    expect(
      screen.getByRole("radio", { name: "Original" }).getAttribute("aria-checked")
    ).toBe("true");
  });

  it("summarizes the selected output once and reports recompression", () => {
    const ready = stagedImage("", {
      kind: "ready",
      variants: [preparedVariant("half", "jpeg", { byte_count: 4096, width: 642, height: 459, savings_percent: 60 })],
      selected: { resize: "half", format: "jpeg" },
      pending: null,
      generation: 1
    });
    const { rerender } = render(dialog([ready]));

    const summary = screen.getByRole("status", { name: "Upload result" });
    expect(summary.textContent).toContain("642");
    expect(summary.textContent).toContain("459");
    expect(summary.textContent).toContain("60% smaller");
    expect(summary.textContent).not.toContain("image/jpeg");

    rerender(
      dialog([
        stagedImage("", {
          kind: "ready",
          variants: [preparedVariant("half", "jpeg", { byte_count: 4096, width: 642, height: 459 })],
          selected: { resize: "quarter", format: "jpeg" },
          pending: { resize: "quarter", format: "jpeg" },
          generation: 2
        })
      ])
    );

    // While recompressing, the summary reports the state instead of numbers
    // that would describe bytes nobody is going to upload.
    const pendingSummary = screen.getByRole("status", { name: "Upload result" });
    expect(pendingSummary.textContent).toContain("Recompressing…");
    expect(pendingSummary.getAttribute("data-upload-output-state")).toBe("recompressing");
    // The preview viewport stays mounted at a stable height.
    const viewport = document.querySelector(".upload-preview-viewport");
    expect(viewport).not.toBeNull();
    expect(viewport?.getAttribute("data-recompressing")).toBe("true");
  });
});

describe("UploadStagingDialog send action", () => {
  function readyItem(stagedId = "staged-1") {
    return stagedImage(
      "",
      {
        kind: "ready",
        variants: [preparedVariant("original", "keep")],
        selected: { resize: "original", format: "keep" },
        pending: null,
        generation: 0
      },
      stagedId
    );
  }

  function sendableDialog(
    items: StagedUploadItem[],
    onSendAttachments = vi.fn()
  ) {
    return (
      <UploadStagingDialog
        items={items}
        onClear={vi.fn()}
        onUpdateCaption={vi.fn()}
        onSelectOutput={vi.fn()}
        onRetryPreparation={vi.fn()}
        onUseOriginal={vi.fn()}
        onSendAttachments={onSendAttachments}
        loadPreview={vi.fn(async () => [])}
      />
    );
  }

  it("owns a distinctly labeled send action for the attachments", () => {
    const onSendAttachments = vi.fn();
    render(sendableDialog([readyItem()], onSendAttachments));

    // A second button literally named "Send" on the same screen is what made
    // the two actions indistinguishable.
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
    const send = screen.getByRole("button", { name: "Send attachments" });
    fireEvent.click(send);
    expect(onSendAttachments).toHaveBeenCalledTimes(1);
  });

  it("refuses to send while an output is still recompressing", () => {
    const onSendAttachments = vi.fn();
    render(
      sendableDialog(
        [
          stagedImage("", {
            kind: "ready",
            variants: [preparedVariant("original", "keep")],
            selected: { resize: "half", format: "keep" },
            pending: { resize: "half", format: "keep" },
            generation: 1
          })
        ],
        onSendAttachments
      )
    );

    const send = screen.getByRole<HTMLButtonElement>("button", {
      name: "Send attachments"
    });
    expect(send.disabled).toBe(true);
    fireEvent.click(send);
    expect(onSendAttachments).not.toHaveBeenCalled();
  });

  it("refuses to send while any attachment is still preparing", () => {
    render(sendableDialog([readyItem(), stagedImage("", { kind: "preparing" }, "staged-2")]));

    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Send attachments" }).disabled
    ).toBe(true);
  });
});
