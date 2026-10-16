// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { HistoryKeyRecoveryControl } from "./HistoryKeyRecoveryControl";

afterEach(cleanup);

test("request acknowledgement comes only from Rust and keeps Retry available", () => {
  const request = vi.fn();
  const view = render(<HistoryKeyRecoveryControl onRequest={request} />);
  fireEvent.click(screen.getByRole("button", { name: "Request decryption keys for past messages" }));
  expect(request).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("Requested")).toBeNull();
  view.rerender(<HistoryKeyRecoveryControl onRequest={request} state={{ generation: 1, status: { kind: "sending" } }} />);
  expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  view.rerender(<HistoryKeyRecoveryControl onRequest={request} state={{ generation: 1, status: { kind: "requested", recipients: 2, responses: 0, received_keys: 0 } }} />);
  expect(screen.getByRole("status").textContent).toBe("Requested");
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(request).toHaveBeenCalledTimes(2);
  expect(screen.getByText(/Element X/)).toBeTruthy();
  view.rerender(<HistoryKeyRecoveryControl onRequest={request} />);
  expect(screen.queryByText("Requested")).toBeNull();
});

test("partial failure remains visible and advanced action names its broad audience", () => {
  render(<HistoryKeyRecoveryControl onRequest={vi.fn()} forceAll state={{ generation: 2, status: { kind: "partial", received_keys: 1, reason: "verificationRequired" } }} />);
  expect(screen.getByText(/Verify/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Request keys from everyone" })).toBeTruthy();
});
