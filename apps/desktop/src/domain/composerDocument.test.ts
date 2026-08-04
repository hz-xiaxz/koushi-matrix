import { describe, expect, it } from "vitest";

import type { MentionTarget } from "./types";
import {
  commitDocument,
  copyDocumentRange,
  createDocumentHistory,
  deleteDocumentBackward,
  deleteDocumentForward,
  documentFromText,
  documentLength,
  insertMention,
  mentionIntentFromDocument,
  moveDocumentCaret,
  pasteDocumentText,
  plainBodyFromDocument,
  redoDocument,
  trimDocument,
  replaceDocumentRange,
  undoDocument,
  type ComposerDocument
} from "./composerDocument";

const alice: MentionTarget = {
  kind: "user",
  user_id: "@alice:example.invalid",
  display_label: "Same Name"
};
const bob: MentionTarget = {
  kind: "user",
  user_id: "@bob:example.invalid",
  display_label: "Same Name"
};

function mixedDocument(): ComposerDocument {
  return insertMention(documentFromText("before  after"), 7, 7, alice, "Same Name");
}

describe("composerDocument", () => {
  it("normalizes text while retaining adjacent and repeated mention identities", () => {
    let document = documentFromText("hello ");
    document = insertMention(document, 6, 6, alice, "Same Name");
    document = insertMention(document, 7, 7, alice, "Same Name");
    document = insertMention(document, 8, 8, bob, "Same Name");

    expect(document.inlines.map((inline) => inline.kind)).toEqual([
      "text",
      "mention",
      "mention",
      "mention"
    ]);
    expect(plainBodyFromDocument(document)).toBe("hello @Same Name@Same Name@Same Name");
    expect(mentionIntentFromDocument(document).targets).toEqual([alice, bob]);
  });

  it("treats Backspace and Delete beside a mention as one atomic transaction", () => {
    const document = mixedDocument();
    const mentionStart = 7;

    const backward = deleteDocumentBackward(document, mentionStart + 1, mentionStart + 1);
    expect(plainBodyFromDocument(backward.document)).toBe("before  after");
    expect(backward.selection).toEqual({ start: mentionStart, end: mentionStart });
    expect(mentionIntentFromDocument(backward.document).targets).toEqual([]);

    const forward = deleteDocumentForward(document, mentionStart, mentionStart);
    expect(plainBodyFromDocument(forward.document)).toBe("before  after");
    expect(forward.selection).toEqual({ start: mentionStart, end: mentionStart });
    expect(mentionIntentFromDocument(forward.document).targets).toEqual([]);
  });

  it("removes every intersected mention for range deletion, cut, and replacement", () => {
    let document = insertMention(documentFromText("A--B"), 1, 2, alice, "Alice");
    document = insertMention(document, 2, 3, bob, "Bob");
    expect(plainBodyFromDocument(document)).toBe("A@Alice@BobB");

    const copied = copyDocumentRange(document, 1, 3);
    expect(copied).toBe("@Alice@Bob");

    const cut = replaceDocumentRange(document, 1, 3, []);
    expect(plainBodyFromDocument(cut)).toBe("AB");
    expect(mentionIntentFromDocument(cut).targets).toEqual([]);

    const replaced = pasteDocumentText(document, 1, 3, "ordinary @Same Name text");
    expect(plainBodyFromDocument(replaced.document)).toBe("Aordinary @Same Name textB");
    expect(mentionIntentFromDocument(replaced.document).targets).toEqual([]);
  });

  it("undoes and redoes visible mentions and metadata together", () => {
    const initial = documentFromText("hello ");
    let history = createDocumentHistory(initial);
    const withMention = insertMention(initial, 6, 6, alice, "Alice");
    history = commitDocument(history, withMention);

    expect(mentionIntentFromDocument(history.present).targets).toEqual([alice]);
    history = undoDocument(history);
    expect(plainBodyFromDocument(history.present)).toBe("hello ");
    expect(mentionIntentFromDocument(history.present).targets).toEqual([]);
    history = redoDocument(history);
    expect(plainBodyFromDocument(history.present)).toBe("hello @Alice");
    expect(mentionIntentFromDocument(history.present).targets).toEqual([alice]);
  });

  it("uses deterministic caret boundaries around atomic mentions", () => {
    const document = mixedDocument();
    expect(documentLength(document)).toBe("before ".length + 1 + " after".length);
    expect(moveDocumentCaret(document, 7, "forward")).toBe(8);
    expect(moveDocumentCaret(document, 8, "backward")).toBe(7);
    expect(moveDocumentCaret(document, 0, "backward")).toBe(0);
    expect(moveDocumentCaret(document, documentLength(document), "forward")).toBe(
      documentLength(document)
    );
  });

  it("trims boundary text without changing mention identity", () => {
    const document = insertMention(documentFromText("  hello @a  "), 8, 10, alice, "Same Name");

    expect(trimDocument(document)).toEqual({
      version: 2,
      inlines: [
        { kind: "text", text: "hello " },
        { kind: "mention", target: alice, display_label: "Same Name" }
      ]
    });
  });

  it("moves across and deletes an emoji grapheme as one text unit", () => {
    const emoji = "👩‍🔬";
    const document = documentFromText(`A${emoji}B`);
    const afterEmoji = 1 + emoji.length;

    expect(moveDocumentCaret(document, 1, "forward")).toBe(afterEmoji);
    expect(moveDocumentCaret(document, afterEmoji, "backward")).toBe(1);
    expect(plainBodyFromDocument(deleteDocumentBackward(document, afterEmoji, afterEmoji).document)).toBe("AB");
    expect(plainBodyFromDocument(deleteDocumentForward(document, 1, 1).document)).toBe("AB");
  });

  it("keeps CJK, emoji, multiline text, and manually matching labels as plain text", () => {
    const text = "研究 👩‍🔬\n@Same Name";
    const document = documentFromText(text);
    expect(plainBodyFromDocument(document)).toBe(text);
    expect(mentionIntentFromDocument(document).targets).toEqual([]);
  });
});
