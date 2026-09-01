import { useRef } from "react";

import { getActiveLocale, t } from "../../i18n/messages";
import { peopleFacingLabel } from "../../app/uiShared";
import {
  FloatingLayer,
  floatingPlacementStyle,
  useFloatingPlacement,
  useHoverFocusPopup
} from "../floatingLayer";
import { renderableThumbnailSourceUrl } from "../../backend/linkMediaRuntime";
import type { LiveReadReceipt } from "../../domain/types";

/** Reader popup width; the panel narrows to the pane when it is smaller. */
const RECEIPT_POPUP_INLINE_SIZE_PX = 260;
/**
 * Reader popup height follows the row count (#360).
 *
 * A fixed height made the popup the same size for two readers as for six, and
 * because `.receipt-tooltip` is a grid, its auto rows stretched to fill the
 * slack — two readers rendered as two ~55px rows with a large blank gap. These
 * mirror the `--receipt-tooltip-*` CSS tokens; keep them in step.
 */
const RECEIPT_POPUP_ROW_BLOCK_SIZE_PX = 17;
const RECEIPT_POPUP_ROW_GAP_PX = 3;
const RECEIPT_POPUP_PADDING_BLOCK_PX = 8;
const RECEIPT_POPUP_BORDER_BLOCK_PX = 1;

function receiptPopupBlockSize(rowCount: number): number {
  const rows = Math.max(rowCount, 1);
  return (
    rows * RECEIPT_POPUP_ROW_BLOCK_SIZE_PX +
    (rows - 1) * RECEIPT_POPUP_ROW_GAP_PX +
    2 * (RECEIPT_POPUP_PADDING_BLOCK_PX + RECEIPT_POPUP_BORDER_BLOCK_PX)
  );
}

/**
 * Read-receipt avatar stack plus its reader popup.
 *
 * The popup renders in the body-level floating layer: the thread pane is
 * overflow-clipped, so a row-local popup gets cut off at the pane edge. Hover
 * and focus open the same popup so keyboard users reach what pointer users see.
 */
export function ReceiptReaders({
  ariaLabel,
  details,
  overflowCount,
  receipts,
  title
}: {
  ariaLabel: string;
  details: string[];
  overflowCount: number;
  receipts: LiveReadReceipt[];
  title: string;
}) {
  const anchorRef = useRef<HTMLDivElement>(null);
  const { open, triggerProps } = useHoverFocusPopup();
  const placement = useFloatingPlacement({
    align: "end",
    anchorRef,
    blockSize: receiptPopupBlockSize(details.length),
    inlineSize: RECEIPT_POPUP_INLINE_SIZE_PX,
    placement: "above",
    resolveBoundaryElement: receiptPopupBoundaryElement
  });

  return (
    <div
      ref={anchorRef}
      className="message-receipts"
      aria-label={ariaLabel}
      tabIndex={0}
      title={title}
      {...triggerProps}
    >
      <span className="receipt-avatars" aria-hidden="true">
        {receipts.map((receipt) => {
          const sourceUrl = receiptAvatarSource(receipt);
          return (
            <span className="receipt-reader-avatar" key={receipt.user_id}>
              {sourceUrl ? (
                <img src={sourceUrl} alt={receiptDisplayName(receipt)} />
              ) : (
                <span dir="auto">{receiptInitials(receipt)}</span>
              )}
            </span>
          );
        })}
        {overflowCount > 0 ? <span className="receipt-overflow">+{overflowCount}</span> : null}
      </span>
      {open && details.length > 0 ? (
        <FloatingLayer>
          <span
            className="receipt-tooltip"
            role="tooltip"
            style={floatingPlacementStyle(placement)}
          >
            {details.map((detail, index) => (
              <span key={`${detail}:${index}`} dir="auto">
                {detail}
              </span>
            ))}
          </span>
        </FloatingLayer>
      ) : null}
    </div>
  );
}

/** Surface the reader popup must stay inside. */
function receiptPopupBoundaryElement(anchor: Element): Element | null {
  return anchor.closest(".thread-pane") ?? anchor.closest(".main-pane");
}

export function formatReceiptDetails(receipts: LiveReadReceipt[], overflowCount: number): string[] {
  const details = receipts.map((receipt) => {
    const timestamp = formatReceiptTimestamp(receipt.timestamp_ms);
    const name = receiptDisplayName(receipt);
    return timestamp ? `${name} ${timestamp}` : name;
  });
  if (overflowCount > 0) {
    details.push(t("timeline.readReceiptOverflow", { count: overflowCount }));
  }
  return details;
}

export function receiptDisplayName(receipt: LiveReadReceipt): string {
  return peopleFacingLabel(receipt.display_name, receipt.original_display_label);
}

function receiptInitials(receipt: LiveReadReceipt): string {
  const label = receiptDisplayName(receipt);
  const ascii = label.match(/[A-Za-z]/g);
  if (ascii?.length) {
    return ascii.slice(0, 2).join("").toUpperCase();
  }
  return label.slice(0, 2);
}

function receiptAvatarSource(receipt: LiveReadReceipt): string | null {
  return receipt.avatar?.thumbnail.kind === "ready"
    ? renderableThumbnailSourceUrl(receipt.avatar.thumbnail.source_ref)
    : null;
}

function formatReceiptTimestamp(timestampMs: number | null): string | null {
  if (timestampMs === null) {
    return null;
  }
  return new Intl.DateTimeFormat(getActiveLocale(), {
    dateStyle: "medium",
    timeStyle: "short"
  }).format(new Date(timestampMs));
}
