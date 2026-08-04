import type {
  ComposerDocument,
  ComposerInline,
  MentionIntent,
  MentionTarget
} from "./types";

export type { ComposerDocument, ComposerInline } from "./types";

export interface DocumentSelection {
  start: number;
  end: number;
}

export interface DocumentMutation {
  document: ComposerDocument;
  selection: DocumentSelection;
}

export interface DocumentHistory {
  past: ComposerDocument[];
  present: ComposerDocument;
  future: ComposerDocument[];
}

export function documentFromText(text: string): ComposerDocument {
  return normalizeDocument({
    version: 2,
    inlines: text ? [{ kind: "text", text }] : []
  });
}

export function normalizeDocument(document: ComposerDocument): ComposerDocument {
  const inlines: ComposerInline[] = [];
  for (const inline of document.inlines) {
    if (inline.kind === "text") {
      if (!inline.text) continue;
      const previous = inlines.at(-1);
      if (previous?.kind === "text") {
        previous.text += inline.text;
      } else {
        inlines.push({ kind: "text", text: inline.text });
      }
    } else {
      inlines.push({
        kind: "mention",
        target: { ...inline.target },
        display_label: inline.display_label
      });
    }
  }
  return { version: 2, inlines };
}

export function documentLength(document: ComposerDocument): number {
  return document.inlines.reduce(
    (length, inline) => length + (inline.kind === "text" ? inline.text.length : 1),
    0
  );
}

export function plainBodyFromDocument(document: ComposerDocument): string {
  return document.inlines
    .map((inline) => (inline.kind === "text" ? inline.text : `@${inline.display_label}`))
    .join("");
}

export function mentionIntentFromDocument(document: ComposerDocument): MentionIntent {
  const targets: MentionTarget[] = [];
  for (const inline of document.inlines) {
    if (
      inline.kind === "mention" &&
      !targets.some((target) => mentionIdentity(target) === mentionIdentity(inline.target))
    ) {
      targets.push({ ...inline.target });
    }
  }
  return { targets };
}

export function insertMention(
  document: ComposerDocument,
  start: number,
  end: number,
  target: MentionTarget,
  displayLabel: string
): ComposerDocument {
  return replaceDocumentRange(document, start, end, [
    { kind: "mention", target: { ...target }, display_label: displayLabel }
  ]);
}

export function replaceDocumentRange(
  document: ComposerDocument,
  start: number,
  end: number,
  replacement: ComposerInline[]
): ComposerDocument {
  const range = normalizedRange(document, start, end);
  const before = splitDocumentAt(document, range.start).before;
  const after = splitDocumentAt(document, range.end).after;
  return normalizeDocument({ version: 2, inlines: [...before, ...replacement, ...after] });
}

export function deleteDocumentBackward(
  document: ComposerDocument,
  start: number,
  end: number
): DocumentMutation {
  const range = normalizedRange(document, start, end);
  const deletionStart = range.start === range.end ? Math.max(0, range.start - 1) : range.start;
  return {
    document: replaceDocumentRange(document, deletionStart, range.end, []),
    selection: { start: deletionStart, end: deletionStart }
  };
}

export function deleteDocumentForward(
  document: ComposerDocument,
  start: number,
  end: number
): DocumentMutation {
  const range = normalizedRange(document, start, end);
  const deletionEnd =
    range.start === range.end
      ? Math.min(documentLength(document), range.end + 1)
      : range.end;
  return {
    document: replaceDocumentRange(document, range.start, deletionEnd, []),
    selection: { start: range.start, end: range.start }
  };
}

export function pasteDocumentText(
  document: ComposerDocument,
  start: number,
  end: number,
  text: string
): DocumentMutation {
  const range = normalizedRange(document, start, end);
  const replacement: ComposerInline[] = text ? [{ kind: "text", text }] : [];
  const next = replaceDocumentRange(document, range.start, range.end, replacement);
  const caret = range.start + text.length;
  return { document: next, selection: { start: caret, end: caret } };
}

export function copyDocumentRange(
  document: ComposerDocument,
  start: number,
  end: number
): string {
  const range = normalizedRange(document, start, end);
  const afterStart = splitDocumentAt(document, range.start).after;
  const selectedLength = range.end - range.start;
  const selected = splitDocumentAt({ version: 2, inlines: afterStart }, selectedLength).before;
  return plainBodyFromDocument({ version: 2, inlines: selected });
}

export function moveDocumentCaret(
  document: ComposerDocument,
  caret: number,
  direction: "backward" | "forward"
): number {
  const length = documentLength(document);
  return Math.max(0, Math.min(length, caret + (direction === "forward" ? 1 : -1)));
}

export function createDocumentHistory(document: ComposerDocument): DocumentHistory {
  return { past: [], present: normalizeDocument(document), future: [] };
}

export function commitDocument(
  history: DocumentHistory,
  document: ComposerDocument
): DocumentHistory {
  const present = normalizeDocument(document);
  if (documentsEqual(history.present, present)) return history;
  return { past: [...history.past, history.present], present, future: [] };
}

export function undoDocument(history: DocumentHistory): DocumentHistory {
  const present = history.past.at(-1);
  if (!present) return history;
  return {
    past: history.past.slice(0, -1),
    present,
    future: [history.present, ...history.future]
  };
}

export function redoDocument(history: DocumentHistory): DocumentHistory {
  const [present, ...future] = history.future;
  if (!present) return history;
  return { past: [...history.past, history.present], present, future };
}

function splitDocumentAt(
  document: ComposerDocument,
  rawOffset: number
): { before: ComposerInline[]; after: ComposerInline[] } {
  const offset = Math.max(0, Math.min(documentLength(document), rawOffset));
  const before: ComposerInline[] = [];
  const after: ComposerInline[] = [];
  let position = 0;
  for (const inline of document.inlines) {
    const length = inline.kind === "text" ? inline.text.length : 1;
    const next = position + length;
    if (next <= offset) {
      before.push(inline);
    } else if (position >= offset) {
      after.push(inline);
    } else if (inline.kind === "text") {
      const local = offset - position;
      if (local > 0) before.push({ kind: "text", text: inline.text.slice(0, local) });
      if (local < inline.text.length) {
        after.push({ kind: "text", text: inline.text.slice(local) });
      }
    } else {
      after.push(inline);
    }
    position = next;
  }
  return { before, after };
}

function normalizedRange(
  document: ComposerDocument,
  start: number,
  end: number
): DocumentSelection {
  const length = documentLength(document);
  const left = Math.max(0, Math.min(length, Math.min(start, end)));
  const right = Math.max(left, Math.min(length, Math.max(start, end)));
  return { start: left, end: right };
}

function mentionIdentity(target: MentionTarget): string {
  switch (target.kind) {
    case "user":
      return `user:${target.user_id}`;
    case "room":
      return `room:${target.room_id}`;
    case "roomMention":
      return "roomMention";
  }
}

function documentsEqual(left: ComposerDocument, right: ComposerDocument): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}
