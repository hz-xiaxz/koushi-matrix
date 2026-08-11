// @vitest-environment jsdom

import { describe, expect, it } from "vitest";

import {
  isUnsupportedSlashCommandRejection,
  type CoreEventPayload
} from "./coreEvents";

describe("isUnsupportedSlashCommandRejection (#450)", () => {
  const submissionRejected: CoreEventPayload = {
    kind: "Timeline",
    event: {
      SubmissionRejected: {
        request_id: { connection_id: 1, sequence: 2 },
        key: {
          account_key: "@a:example.invalid",
          kind: { Room: { room_id: "!r:example.invalid" } }
        },
        submission_id: "submission-1",
        kind: "UnsupportedSlashCommand"
      }
    }
  };
  const operationFailed: CoreEventPayload = {
    kind: "OperationFailed",
    request_id: null,
    failure: {
      TimelineOperationFailed: { kind: "UnsupportedSlashCommand" }
    }
  };

  it("recognizes the production SubmissionRejected surface", () => {
    expect(isUnsupportedSlashCommandRejection(submissionRejected)).toBe(true);
  });

  it("recognizes the OperationFailed surface", () => {
    expect(isUnsupportedSlashCommandRejection(operationFailed)).toBe(true);
  });

  it("ignores other rejection kinds and unrelated payloads", () => {
    expect(
      isUnsupportedSlashCommandRejection({
        kind: "Timeline",
        event: {
          SubmissionRejected: {
            request_id: { connection_id: 1, sequence: 2 },
            key: {
              account_key: "@a:example.invalid",
              kind: { Room: { room_id: "!r:example.invalid" } }
            },
            submission_id: "submission-1",
            kind: "Network"
          }
        }
      } as CoreEventPayload)
    ).toBe(false);
    expect(
      isUnsupportedSlashCommandRejection({
        kind: "OperationFailed",
        request_id: null,
        failure: { TimelineOperationFailed: { kind: "Forbidden" } }
      })
    ).toBe(false);
    expect(
      isUnsupportedSlashCommandRejection({
        kind: "OperationFailed",
        request_id: null,
        failure: { TimelineOperationFailed: { kind: "UnsupportedSlashCommand" } }
      })
    ).toBe(true);
    expect(
      isUnsupportedSlashCommandRejection({
        kind: "Sync",
        event: { Connected: { kind: "connecting" } }
      } as unknown as CoreEventPayload)
    ).toBe(false);
  });
});
