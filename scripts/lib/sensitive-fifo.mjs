import { constants } from "node:fs";
import { open } from "node:fs/promises";
import { performance } from "node:perf_hooks";

const OPEN_RETRY_MS = 10;

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

export async function writeSensitivePayloadToPath(path, payload, timeout) {
  const deadline = performance.now() + timeout;
  while (performance.now() < deadline) {
    let handle;
    try {
      handle = await open(path, constants.O_WRONLY | constants.O_NONBLOCK);
    } catch (error) {
      if (error?.code !== "ENXIO") {
        throw error;
      }
      await sleep(Math.min(OPEN_RETRY_MS, Math.max(0, deadline - performance.now())));
      continue;
    }

    try {
      if (performance.now() >= deadline) {
        throw new Error("sensitive FIFO write timed out");
      }
      await handle.writeFile(payload, "utf8");
      return;
    } finally {
      await handle.close();
    }
  }
  throw new Error("sensitive FIFO write timed out");
}
