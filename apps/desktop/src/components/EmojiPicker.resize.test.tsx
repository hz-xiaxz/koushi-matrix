// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { EmojiPicker } from "./EmojiPicker";

afterEach(cleanup);
it("offers keyboard resizing and commits the requested size", () => {
  const onSizeChange = vi.fn();
  render(<EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} size={[400, 400]} onSizeChange={onSizeChange} />);
  const handle = screen.getByRole("button", { name: /resize emoji picker/i });
  fireEvent.keyDown(handle, { key: "ArrowRight" });
  expect(onSizeChange).toHaveBeenLastCalledWith([420, 400]);
  fireEvent.keyDown(handle, { key: "ArrowDown" });
  expect(onSizeChange).toHaveBeenLastCalledWith([420, 420]);
});
