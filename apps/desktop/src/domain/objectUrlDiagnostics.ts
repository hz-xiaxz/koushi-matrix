// Accounting follows the existing presentation owner's lifetime. Only numeric
// totals leave this module; URLs and Blob contents are never diagnostic output.
const liveUrls = new Map<string, number>();

export function createTrackedObjectURL(blob: Blob): string {
  const url = URL.createObjectURL(blob);
  liveUrls.set(url, blob.size);
  return url;
}

export function revokeTrackedObjectURL(url: string): void {
  URL.revokeObjectURL(url);
  liveUrls.delete(url);
}

export function objectUrlDiagnostics(): { objectUrls: number; objectUrlBlobBytes: number } {
  let objectUrlBlobBytes = 0;
  for (const bytes of liveUrls.values()) objectUrlBlobBytes += bytes;
  return { objectUrls: liveUrls.size, objectUrlBlobBytes };
}
