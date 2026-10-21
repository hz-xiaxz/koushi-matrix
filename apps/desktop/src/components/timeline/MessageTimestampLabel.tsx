import { useActiveLocaleKey } from "../../i18n/activeLocaleContext";
import { Tooltip } from "../Tooltip";
import { formatFullMessageTimestamp, formatMessageTimestamp, type MessageTimestampStyle } from "./messageTimestamp";
import { useCalendarPresentation } from "./useCalendarPresentation";

/** Shared focusable timestamp, including the full local date and time. */
export function MessageTimestamp({ timestampMs, style = "relative" }: {
  timestampMs: number | null;
  style?: MessageTimestampStyle;
}) {
  useActiveLocaleKey();
  useCalendarPresentation();
  const label = formatMessageTimestamp(timestampMs, Date.now(), style);
  if (timestampMs === null || label === null) return null;
  return (
    <Tooltip label={formatFullMessageTimestamp(timestampMs)} placement="above" openOnFocus>
      {(tooltipProps) => (
        <time className="message-timestamp" dateTime={new Date(timestampMs).toISOString()} tabIndex={0} {...tooltipProps}>
          {label}
        </time>
      )}
    </Tooltip>
  );
}
