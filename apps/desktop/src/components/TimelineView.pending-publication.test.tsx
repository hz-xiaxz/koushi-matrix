// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { CoreEventPayload, TimelineItem } from "../domain/coreEvents";
import { TimelineView } from "./TimelineView";
import { KEY, baseTransport, message } from "./timelineViewTestSupport";

afterEach(cleanup);

test("Rust pending publication survives unrelated updates and converges to one sent row", async () => {
  let emit: (event: CoreEventPayload) => void = () => undefined;
  const editMessage = vi.fn();
  const transport = baseTransport({
    listenCoreEvents(listener) { emit = listener; return () => undefined; },
    editMessage
  });
  render(<TimelineView timelineKey={KEY} roomId="!room:example.invalid" transport={transport} onReply={vi.fn()} />);
  act(() => emit({ kind: "Timeline", event: { InitialItems: { request_id: null, key: KEY, generation: 1, items: [] } } }));
  const pending: TimelineItem = {
    ...message("$unused", "Synthetic pending row"),
    id: { Transaction: { transaction_id: "synthetic-pending" } },
    sender: "@alice:example.invalid", send_state: { kind: "sending" }, can_react: false
  };
  act(() => emit({ kind: "Timeline", event: { ItemsUpdated: { key: KEY, generation: 1, batch_id: 0, diffs: [{ PushBack: { item: pending } }] } } }));
  expect(await screen.findByText("Synthetic pending row")).toBeTruthy();
  act(() => emit({ kind: "Timeline", event: { ItemsUpdated: { key: KEY, generation: 1, batch_id: 1, diffs: [{ PushBack: { item: message("$incoming:example.invalid", "Synthetic incoming row") } }] } } }));
  expect(await screen.findByText("Synthetic incoming row")).toBeTruthy();
  expect(screen.getAllByText("Synthetic pending row")).toHaveLength(1);
  const sent: TimelineItem = { ...message("$sent:example.invalid", "Synthetic pending row"), sender: pending.sender, send_state: { kind: "sent" } };
  act(() => emit({ kind: "Timeline", event: { ItemsUpdated: { key: KEY, generation: 1, batch_id: 2, diffs: [{ Set: { index: 0, item: sent } }] } } }));
  expect(screen.getAllByText("Synthetic pending row")).toHaveLength(1);
  expect(editMessage).not.toHaveBeenCalled();
});
