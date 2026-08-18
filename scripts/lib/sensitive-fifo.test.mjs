import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { constants } from "node:fs";
import { mkdtemp, open, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { writeSensitivePayloadToPath } from "./sensitive-fifo.mjs";

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

test(
  "a timed-out FIFO writer cannot write later when a reader appears",
  { skip: process.platform === "win32" },
  async () => {
    const directory = await mkdtemp(path.join(os.tmpdir(), "koushi-fifo-"));
    const fifo = path.join(directory, "credentials.pipe");
    execFileSync("mkfifo", [fifo]);

    try {
      await assert.rejects(
        writeSensitivePayloadToPath(fifo, "synthetic-secret\n", 20),
        /timed out/
      );

      const reader = await open(fifo, constants.O_RDONLY | constants.O_NONBLOCK);
      try {
        const buffer = Buffer.alloc(64);
        let bytesRead = 0;
        for (let attempt = 0; attempt < 20 && bytesRead === 0; attempt += 1) {
          try {
            ({ bytesRead } = await reader.read(buffer, 0, buffer.length, null));
          } catch (error) {
            if (error?.code !== "EAGAIN") {
              throw error;
            }
          }
          if (bytesRead === 0) {
            await sleep(10);
          }
        }
        assert.equal(bytesRead, 0, "timed-out credential payload must never be written later");
      } finally {
        await reader.close();
      }
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  }
);
