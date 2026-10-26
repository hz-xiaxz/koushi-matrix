// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { avatarImageDiagnostics } from "../domain/avatarImageDiagnostics";
import { useRecoverableImageSource } from "./avatarImage";

afterEach(cleanup);

test("image diagnostics survive fallback and unmount without retaining sources", () => {
  const before = avatarImageDiagnostics();
  const { result, unmount } = renderHook(() => useRecoverableImageSource("https://synthetic.invalid/private-avatar"));
  act(() => result.current.onImageLoad());
  act(() => result.current.onImageError());
  expect(result.current.displaySourceUrl).toBeNull();
  unmount();
  const after = avatarImageDiagnostics();
  expect(after.loads - before.loads).toBe(1);
  expect(after.errors - before.errors).toBe(1);
  expect(JSON.stringify(after)).not.toContain("private-avatar");
  expect(avatarImageDiagnostics()).toEqual(after);
});
