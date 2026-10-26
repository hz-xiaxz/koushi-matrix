// Numeric page-lifetime event totals only. Never retain a source or DOM event.
let loads = 0;
let errors = 0;

export function recordAvatarImageEvent(event: "load" | "error"): void {
  if (event === "load") loads = Math.min(Number.MAX_SAFE_INTEGER, loads + 1);
  else errors = Math.min(Number.MAX_SAFE_INTEGER, errors + 1);
}

export function avatarImageDiagnostics(): { loads: number; errors: number } {
  return { loads, errors };
}
