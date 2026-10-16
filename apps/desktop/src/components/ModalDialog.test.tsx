// @vitest-environment jsdom

import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { t } from "../i18n/messages";
import { ModalDialog } from "./ModalDialog";

afterEach(() => {
  cleanup();
});

describe("ModalDialog", () => {
  it("provides a primary-button-only window drag region over the macOS title-bar block", () => {
    const onStartWindowDrag = vi.fn();
    const { container } = render(
      <ModalDialog
        title={t("diagnostics.title")}
        onClose={() => undefined}
        onStartWindowDrag={onStartWindowDrag}
      >
        {null}
      </ModalDialog>
    );

    const dragRegion = container.querySelector(".native-modal-drag-region");
    expect(dragRegion).toBeTruthy();
    expect(dragRegion?.getAttribute("data-tauri-drag-region")).toBe("");
    fireEvent.mouseDown(dragRegion!, { button: 2, buttons: 2 });
    expect(onStartWindowDrag).not.toHaveBeenCalled();
    fireEvent.mouseDown(dragRegion!, { button: 0, buttons: 1 });
    expect(onStartWindowDrag).toHaveBeenCalledTimes(1);
  });
});
