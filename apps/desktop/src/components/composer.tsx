import {
  type ChangeEvent,
  type DragEvent,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  memo,
  useEffect,
  useId,
  useRef,
  useState
} from "react";
import {
  AtSign,
  Bold,
  Clock3,
  Code2,
  Italic,
  Link2,
  List,
  Paperclip,
  Send,
  Sigma,
  Smile,
  X
} from "lucide-react";
import { t } from "../i18n/messages";
import type {
  ComposerDocument,
  ComposerSurface,
  ResolveComposerKeyAction
} from "../domain/types";
import {
  IS_MAC_PLATFORM,
  applyMacEmacsAction,
  composerKeyEventFromDom,
  macEmacsActionFromEvent,
  shouldLetNativeImeHandleComposerKeyEvent,
  shouldResolveComposerKeyEvent
} from "../domain/composerKeyEvents";
import { EmojiPicker } from "./EmojiPicker";
import {
  ImeInlineMentionEditor,
  type ImeInlineMentionEditorHandle,
  ImeSafeForm,
} from "./ImeTextControl";
import {
  ICON_SIZE,
  ignoreComposerKeyAction,
  activeMentionQuery,
  mentionTargetKey,
  peopleFacingLabel,
  initials,
  defaultScheduleDateTimeValue,
  scheduledSendTimestampFromInput,
  type MentionCandidate,
  type ComposerModeProp
} from "../app/uiShared";
import { EntityAvatar } from "./Shell";
import {
  attachmentTransferHasFiles,
  filesFromAttachmentTransfer,
  ingestAttachmentFiles
} from "../domain/attachmentIngestion";
import {
  copyDocumentRange,
  documentLength,
  insertMention,
  pasteDocumentText,
  plainBodyFromDocument,
  replaceDocumentRange,
  type DocumentSelection
} from "../domain/composerDocument";

export const Composer = memo(function Composer({
  surface = "main",
  editorOnly = false,
  canEdit = true,
  composerMode,
  hasStagedUploads = false,
  isSending,
  mathModeEnabled = true,
  mentionCandidates = [],
  mentionCandidatesLoading = false,
  resolveComposerKeyAction = ignoreComposerKeyAction,
  draftKey = "default",
  ariaLabel = t("composer.messageComposer"),
  document,
  placeholder,
  roomName,
  onCancel,
  onCancelReply,
  onAttachFiles = async () => undefined,
  onDocumentChange,
  onMathModeChange = () => undefined,
  onMentionQueryChange = () => undefined,
  onScheduleSend,
  onSend,
  notice = null
}: {
  surface?: ComposerSurface;
  editorOnly?: boolean;
  canEdit?: boolean;
  composerMode: ComposerModeProp;
  hasStagedUploads?: boolean;
  isSending: boolean;
  mathModeEnabled?: boolean;
  mentionCandidates?: MentionCandidate[];
  mentionCandidatesLoading?: boolean;
  resolveComposerKeyAction?: ResolveComposerKeyAction;
  draftKey?: string;
  ariaLabel?: string;
  document: ComposerDocument;
  placeholder?: string;
  roomName: string;
  onCancel?: () => void;
  onCancelReply: () => void;
  onAttachFiles?: (files: File[]) => void | Promise<void>;
  onDocumentChange: (document: ComposerDocument) => void;
  onMathModeChange?: (enabled: boolean) => void | Promise<void>;
  onMentionQueryChange?: (query: string | null) => void;
  onScheduleSend?: (sendAtMs: number, document: ComposerDocument) => void | Promise<void>;
  onSend: (document: ComposerDocument) => void | Promise<void>;
  /** Localized transient notice rendered above the composer (issue #450). */
  notice?: string | null;
}) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const emojiButtonRef = useRef<HTMLButtonElement>(null);
  const macKillRingRef = useRef<string>("");
  const onMentionQueryChangeRef = useRef(onMentionQueryChange);
  onMentionQueryChangeRef.current = onMentionQueryChange;
  const [scheduleOpen, setScheduleOpen] = useState(false);
  const [emojiPickerOpen, setEmojiPickerOpen] = useState(false);
  const [scheduleValue, setScheduleValue] = useState(() => defaultScheduleDateTimeValue());
  const [localDocument, setLocalDocument] = useState(document);
  const [localDraftKey, setLocalDraftKey] = useState(draftKey);
  const [documentSelection, setDocumentSelection] = useState<DocumentSelection>(() => {
    const end = documentLength(document);
    return { start: end, end };
  });
  const [activeMentionIndex, setActiveMentionIndex] = useState(0);
  const [dismissedMentionKey, setDismissedMentionKey] = useState<string | null>(null);
  const [fileDragActive, setFileDragActive] = useState(false);
  const editorRef = useRef<ImeInlineMentionEditorHandle>(null);
  const documentEpochRef = useRef(0);
  const keyResolutionPendingRef = useRef(false);
  const mountedRef = useRef(true);
  const autocompleteListboxId = useId();
  if (localDraftKey !== draftKey) {
    const end = documentLength(document);
    setLocalDraftKey(draftKey);
    setLocalDocument(document);
    setDocumentSelection({ start: end, end });
    documentEpochRef.current += 1;
  }
  const localValue = plainBodyFromDocument(localDocument);
  const mentionQueryText = localDocument.inlines
    .map((inline) => (inline.kind === "text" ? inline.text : "\uFFFC"))
    .join("");
  const activeMention =
    documentSelection.start === documentSelection.end
      ? activeMentionQuery(mentionQueryText.slice(0, documentSelection.end))
      : null;
  const activeMentionKey =
    activeMention === null ? null : `${activeMention.start}:${activeMention.query.toLowerCase()}`;
  const activeMentionSuggestions =
    activeMention === null || activeMentionKey === dismissedMentionKey
      ? []
      : mentionCandidates;
  const autocompleteOpen =
    activeMention !== null &&
    activeMentionKey !== dismissedMentionKey &&
    (activeMentionSuggestions.length > 0 || mentionCandidatesLoading);
  const activeMentionOption = autocompleteOpen
    ? activeMentionSuggestions[Math.min(activeMentionIndex, activeMentionSuggestions.length - 1)]
    : undefined;
  const activeMentionOptionId =
    autocompleteOpen && activeMentionOption
      ? `${autocompleteListboxId}-option-${Math.min(activeMentionIndex, activeMentionSuggestions.length - 1)}`
      : undefined;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    setLocalDocument(document);
    const end = documentLength(document);
    setDocumentSelection({ start: end, end });
    documentEpochRef.current += 1;
  }, [document]);

  useEffect(() => {
    setActiveMentionIndex(0);
  }, [activeMentionKey]);

  useEffect(() => {
    onMentionQueryChangeRef.current(activeMention?.query ?? null);
  }, [activeMentionKey]);

  useEffect(() => {
    setActiveMentionIndex((current) =>
      activeMentionSuggestions.length === 0
        ? 0
        : Math.min(current, activeMentionSuggestions.length - 1)
    );
  }, [activeMentionSuggestions.length]);

  function updateLocalDocument(nextDocument: ComposerDocument, selection?: DocumentSelection) {
    documentEpochRef.current += 1;
    setLocalDocument(nextDocument);
    if (selection) setDocumentSelection(selection);
    onDocumentChange(nextDocument);
  }

  function commitEditorMutation(mutation: { document: ComposerDocument; selection: DocumentSelection }) {
    if (editorRef.current) {
      editorRef.current.commit(mutation);
      return;
    }
    updateLocalDocument(mutation.document, mutation.selection);
  }

  function replaceTextRange(
    start: number,
    end: number,
    replacement: string,
    cursorOffset = replacement.length
  ) {
    const nextDocument = replaceDocumentRange(
      localDocument,
      start,
      end,
      replacement ? [{ kind: "text", text: replacement }] : []
    );
    const cursor = start + cursorOffset;
    commitEditorMutation({ document: nextDocument, selection: { start: cursor, end: cursor } });
    requestAnimationFrame(() => {
      editorRef.current?.focus();
    });
  }

  function closeAutocompleteForCurrentQuery() {
    if (activeMentionKey) {
      setDismissedMentionKey(activeMentionKey);
    }
  }

  function acceptActiveMention() {
    const candidate =
      activeMentionSuggestions[Math.min(activeMentionIndex, activeMentionSuggestions.length - 1)];
    if (candidate) {
      acceptMention(candidate);
    }
  }

  function selectionRange(): DocumentSelection {
    return editorRef.current?.selection() ?? documentSelection;
  }

  function keepComposerFocus(event: MouseEvent<HTMLButtonElement>) {
    event.preventDefault();
  }

  function applyInlineMarkdown(prefix: string, suffix = prefix, placeholder = "") {
    const { start, end } = selectionRange();
    const selected = copyDocumentRange(localDocument, start, end) || placeholder;
    replaceTextRange(
      start,
      end,
      `${prefix}${selected}${suffix}`,
      prefix.length + selected.length + suffix.length
    );
  }

  function applyLinkMarkdown() {
    const { start, end } = selectionRange();
    const selected = copyDocumentRange(localDocument, start, end) || "link";
    const replacement = `[${selected}](https://)`;
    replaceTextRange(start, end, replacement, replacement.length - 1);
  }

  function applyListMarkdown() {
    const { start, end } = selectionRange();
    const selected = copyDocumentRange(localDocument, start, end);
    if (!selected) {
      replaceTextRange(start, end, "- ", 2);
      return;
    }
    const replacement = selected
      .split("\n")
      .map((line) => (line.startsWith("- ") ? line : `- ${line}`))
      .join("\n");
    replaceTextRange(start, end, replacement);
  }

  function insertMentionTrigger() {
    const { start, end } = selectionRange();
    replaceTextRange(start, end, "@");
  }

  function insertEmoji(emoji: string) {
    const { start, end } = selectionRange();
    replaceTextRange(start, end, emoji);
  }

  function acceptMention(candidate: MentionCandidate) {
    if (!activeMention) {
      return;
    }
    const displayLabel = peopleFacingLabel(candidate.label);
    const target =
      candidate.target.kind === "user"
        ? { ...candidate.target, display_label: displayLabel }
        : candidate.target;
    const withMention = insertMention(
      localDocument,
      activeMention.start,
      activeMention.end,
      target,
      displayLabel
    );
    const nextDocument = pasteDocumentText(
      withMention,
      activeMention.start + 1,
      activeMention.start + 1,
      " "
    ).document;
    const cursor = activeMention.start + 2;
    commitEditorMutation({ document: nextDocument, selection: { start: cursor, end: cursor } });
    requestAnimationFrame(() => {
      editorRef.current?.focus();
    });
  }

  async function onAttachFileChange(event: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(event.currentTarget.files ?? []);
    event.currentTarget.value = "";
    try {
      await ingestAttachmentFiles(files, onAttachFiles);
    } catch {
      // Upload failure is reported through the Rust-owned operation/event path.
    }
  }

  async function attachDroppedOrPastedFiles(files: File[]) {
    if (!canEdit) {
      return;
    }
    try {
      await ingestAttachmentFiles(files, onAttachFiles);
    } catch {
      // Upload failure is reported through the Rust-owned operation/event path.
    }
  }

  function onAttachmentDragEnter(event: DragEvent<HTMLElement>) {
    if (!canEdit || !attachmentTransferHasFiles(event.dataTransfer)) {
      return;
    }
    event.preventDefault();
    setFileDragActive(true);
  }

  function onAttachmentDragOver(event: DragEvent<HTMLElement>) {
    if (!canEdit || !attachmentTransferHasFiles(event.dataTransfer)) {
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
    setFileDragActive(true);
  }

  function onAttachmentDragLeave(event: DragEvent<HTMLElement>) {
    const nextTarget = event.relatedTarget;
    if (nextTarget instanceof Node && event.currentTarget.contains(nextTarget)) {
      return;
    }
    setFileDragActive(false);
  }

  function onAttachmentDrop(event: DragEvent<HTMLElement>) {
    event.preventDefault();
    setFileDragActive(false);
    void attachDroppedOrPastedFiles(filesFromAttachmentTransfer(event.dataTransfer));
  }

  function openScheduleForm() {
    setScheduleValue(defaultScheduleDateTimeValue());
    setScheduleOpen(true);
  }

  async function submitSchedule(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const sendAtMs = scheduledSendTimestampFromInput(scheduleValue);
    if (sendAtMs === null || !localValue.trim() || hasStagedUploads || isSending) {
      return;
    }
    await onScheduleSend?.(sendAtMs, localDocument);
    setScheduleOpen(false);
  }

  function onComposerKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (IS_MAC_PLATFORM && !event.nativeEvent.isComposing) {
      const emacsAction = macEmacsActionFromEvent(event);
      if (emacsAction !== null) {
        event.preventDefault();
        const range = selectionRange();
        if (emacsAction === "killToEol") {
          const lineEnd = mentionQueryText.indexOf("\n", range.start);
          const end = lineEnd === -1 ? documentLength(localDocument) : lineEnd;
          macKillRingRef.current = copyDocumentRange(localDocument, range.start, end);
          const next = replaceDocumentRange(localDocument, range.start, end, []);
          commitEditorMutation({
            document: next,
            selection: { start: range.start, end: range.start }
          });
          return;
        }
        if (emacsAction === "yank") {
          const mutation = pasteDocumentText(
            localDocument,
            range.start,
            range.end,
            macKillRingRef.current
          );
          commitEditorMutation(mutation);
          return;
        }
        const effect = applyMacEmacsAction(
          emacsAction,
          mentionQueryText,
          range.start,
          range.end,
          macKillRingRef.current
        );
        if (effect && editorRef.current) {
          editorRef.current.setSelection({
            start: effect.newSelectionPos,
            end: effect.newSelectionPos
          });
          setDocumentSelection({ start: effect.newSelectionPos, end: effect.newSelectionPos });
        }
        return;
      }
    }
    if (autocompleteOpen) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const direction = event.key === "ArrowDown" ? 1 : -1;
        setActiveMentionIndex((current) =>
          (current + direction + activeMentionSuggestions.length) % activeMentionSuggestions.length
        );
        return;
      }
      if (event.key === "Tab") {
        event.preventDefault();
        acceptActiveMention();
        return;
      }
    }
    if (!shouldResolveComposerKeyEvent(event)) {
      return;
    }

    if (keyResolutionPendingRef.current) return;
    const intentDocument = localDocument;
    const intentValue = localValue;
    const intentSelection = selectionRange();
    const intentEpoch = documentEpochRef.current;
    const keyEvent = composerKeyEventFromDom(event, intentSelection);
    const resolverOptions = {
      autocomplete_open: autocompleteOpen,
      // Text-only: staged attachments are sent from the staging panel, so
      // Enter must never dispatch them implicitly.
      send_enabled: canEdit && !isSending && intentValue.trim().length > 0
    };
    if (shouldLetNativeImeHandleComposerKeyEvent(keyEvent)) {
      void resolveComposerKeyAction(surface, keyEvent, resolverOptions).catch(() => undefined);
      return;
    }
    event.preventDefault();
    keyResolutionPendingRef.current = true;

    void resolveComposerKeyAction(surface, keyEvent, resolverOptions)
      .then((action) => {
        if (!mountedRef.current) return;
        if (action === "send") {
          void onSend(intentDocument);
          return;
        }
        if (documentEpochRef.current !== intentEpoch) return;
        if (action === "insertNewline") {
          const mutation = pasteDocumentText(
            intentDocument,
            intentSelection.start,
            intentSelection.end,
            "\n"
          );
          commitEditorMutation(mutation);
          return;
        }
        if (action === "acceptAutocomplete") {
          acceptActiveMention();
          return;
        }
        if (action === "closeAutocomplete") {
          closeAutocompleteForCurrentQuery();
          return;
        }
        if (action === "cancel") {
          if (composerMode.kind === "reply") onCancelReply();
          else onCancel?.();
        }
      })
      .catch(() => undefined)
      .finally(() => {
        keyResolutionPendingRef.current = false;
      });
  }

  return (
    <section
      className={`composer${editorOnly ? " is-editor-only" : ""}${fileDragActive ? " is-file-drag-over" : ""}`}
      aria-label={ariaLabel}
      data-file-drag-over={fileDragActive ? "true" : "false"}
      onDragEnter={onAttachmentDragEnter}
      onDragOver={onAttachmentDragOver}
      onDragLeave={onAttachmentDragLeave}
      onDrop={onAttachmentDrop}
    >
      <div className="composer-drop-overlay" aria-hidden={!fileDragActive}>
        {t("composer.dropFiles")}
      </div>
      {notice ? (
        <p className="composer-notice" role="status">
          {notice}
        </p>
      ) : null}
      {!editorOnly && composerMode.kind === "reply" ? (
        <div className="composer-reply-banner">
          <span className="composer-reply-label">{t("composer.replying")}</span>
          <button
            className="icon-button"
            type="button"
            aria-label={t("composer.cancelReply")}
            onClick={onCancelReply}
          >
            <X size={ICON_SIZE.small} />
          </button>
        </div>
      ) : null}
      <div className="composer-tools">
        <button
          className="icon-button"
          type="button"
          aria-label={t("composer.bold")}
          onMouseDown={keepComposerFocus}
          onClick={() => applyInlineMarkdown("**", "**", "bold")}
        >
          <Bold size={ICON_SIZE.input} />
        </button>
        <button
          className="icon-button"
          type="button"
          aria-label={t("composer.italic")}
          onMouseDown={keepComposerFocus}
          onClick={() => applyInlineMarkdown("_", "_", "italic")}
        >
          <Italic size={ICON_SIZE.input} />
        </button>
        <button
          className="icon-button"
          type="button"
          aria-label={t("composer.link")}
          onMouseDown={keepComposerFocus}
          onClick={applyLinkMarkdown}
        >
          <Link2 size={ICON_SIZE.input} />
        </button>
        <button
          className="icon-button"
          type="button"
          aria-label={t("composer.list")}
          onMouseDown={keepComposerFocus}
          onClick={applyListMarkdown}
        >
          <List size={ICON_SIZE.input} />
        </button>
        <button
          className="icon-button"
          type="button"
          aria-label={t("composer.code")}
          onMouseDown={keepComposerFocus}
          onClick={() => applyInlineMarkdown("`", "`", "code")}
        >
          <Code2 size={ICON_SIZE.input} />
        </button>
        {/* #453: this is an on/off switch for `SettingsValues.composer.math_mode`,
            not an insert-markup action like the buttons beside it. It speaks the
            same switch vocabulary as the settings panels so the state is
            readable at rest. */}
        <button
          className="composer-math-toggle"
          type="button"
          role="switch"
          aria-checked={mathModeEnabled}
          aria-label={mathModeEnabled ? t("composer.mathModeOn") : t("composer.mathModeOff")}
          title={mathModeEnabled ? t("composer.mathModeOn") : t("composer.mathModeOff")}
          onMouseDown={keepComposerFocus}
          onClick={() => {
            void onMathModeChange(!mathModeEnabled);
          }}
        >
          <Sigma size={ICON_SIZE.input} aria-hidden="true" />
          <span>{t("composer.mathMode")}</span>
          <span className="composer-math-switch-track" aria-hidden="true">
            <span className="composer-math-switch-thumb" />
          </span>
        </button>
      </div>
      <MentionAutocomplete
        open={autocompleteOpen}
        listboxId={autocompleteListboxId}
        activeIndex={activeMentionIndex}
        candidates={activeMentionSuggestions}
        loading={mentionCandidatesLoading}
        activeOptionId={activeMentionOptionId}
        onAccept={acceptMention}
        onMouseDown={keepComposerFocus}
      />
      <ImeInlineMentionEditor
        ref={editorRef}
        aria-label={ariaLabel}
        className="composer-inline-editor"
        data-placeholder={placeholder ?? t("composer.placeholder", { roomName })}
        document={localDocument}
        editable={canEdit}
        syncKey={draftKey}
        onDocumentChange={(nextDocument) => updateLocalDocument(nextDocument)}
        onSelectionChange={setDocumentSelection}
        onKeyDown={onComposerKeyDown}
        onPaste={(event) => {
          const files = filesFromAttachmentTransfer(event.clipboardData);
          if (files.length > 0) {
            event.preventDefault();
            void attachDroppedOrPastedFiles(files);
          }
        }}
      />
      {!editorOnly ? <div className="composer-footer">
        <div>
          <input
            ref={fileInputRef}
            className="composer-file-input"
            type="file"
            multiple
            aria-label={t("composer.attachFileInput")}
            onChange={(event) => {
              void onAttachFileChange(event);
            }}
          />
          <button
            className="icon-button"
            type="button"
            aria-label={t("composer.attachFile")}
            disabled={!canEdit}
            onClick={() => fileInputRef.current?.click()}
          >
            <Paperclip size={ICON_SIZE.control} />
          </button>
          <button
            className="icon-button"
            type="button"
            aria-label={t("composer.mention")}
            onMouseDown={keepComposerFocus}
            onClick={insertMentionTrigger}
          >
            <AtSign size={ICON_SIZE.control} />
          </button>
          <span className="composer-emoji-anchor">
            <button
              ref={emojiButtonRef}
              className="icon-button"
              type="button"
              aria-label={t("composer.emoji")}
              aria-expanded={emojiPickerOpen}
              aria-haspopup="dialog"
              onClick={() => setEmojiPickerOpen((open) => !open)}
            >
              <Smile size={ICON_SIZE.control} />
            </button>
            {emojiPickerOpen ? (
              <EmojiPicker
                anchorRef={emojiButtonRef}
                onSelect={insertEmoji}
                onClose={() => setEmojiPickerOpen(false)}
              />
            ) : null}
          </span>
          {onScheduleSend ? (
            <button
              className="icon-button"
              type="button"
              aria-label={t("scheduled.sendLater")}
              disabled={!canEdit || isSending || !localValue.trim() || hasStagedUploads}
              onClick={openScheduleForm}
            >
              <Clock3 size={ICON_SIZE.control} />
            </button>
          ) : null}
        </div>
        <button
          className={`send-button ${localValue.trim() && !isSending ? "ready" : ""} ${isSending ? "is-sending" : ""}`}
          type="button"
          aria-label={isSending ? t("action.sending") : t("action.send")}
          disabled={!canEdit || isSending || !localValue.trim()}
          onClick={() => onSend(localDocument)}
        >
          <Send size={ICON_SIZE.input} />
        </button>
      </div> : null}
      {scheduleOpen && onScheduleSend ? (
        <ImeSafeForm className="scheduled-send-form" onSubmit={submitSchedule}>
          <label className="scheduled-send-field">
            <span>{t("scheduled.timeInput")}</span>
            <input
              aria-label={t("scheduled.timeInput")}
              type="datetime-local"
              value={scheduleValue}
              onChange={(event) => setScheduleValue(event.currentTarget.value)}
            />
          </label>
          <div className="scheduled-send-form-actions">
            <button className="dialog-button" type="button" onClick={() => setScheduleOpen(false)}>
              {t("action.cancel")}
            </button>
            <button
              className="dialog-button is-primary"
              type="submit"
              disabled={scheduledSendTimestampFromInput(scheduleValue) === null}
            >
              {t("scheduled.schedule")}
            </button>
          </div>
        </ImeSafeForm>
      ) : null}
    </section>
  );
});

type MentionSection = {
  key: "users" | "room";
  label: string;
  candidates: Array<{ candidate: MentionCandidate; index: number }>;
};

function mentionSections(candidates: MentionCandidate[]): MentionSection[] {
  const users: MentionSection["candidates"] = [];
  const roomMentions: MentionSection["candidates"] = [];
  candidates.forEach((candidate, index) => {
    const item = { candidate, index };
    if (candidate.target.kind === "roomMention") {
      roomMentions.push(item);
    } else {
      users.push(item);
    }
  });
  return [
    ...(users.length ? [{ key: "users" as const, label: t("composer.mentionUsers"), candidates: users }] : []),
    ...(roomMentions.length
      ? [
          {
            key: "room" as const,
            label: t("composer.mentionRoomNotification"),
            candidates: roomMentions
          }
        ]
      : [])
  ];
}

function MentionOption({
  active,
  candidate,
  id,
  onAccept,
  onMouseDown
}: {
  active: boolean;
  candidate: MentionCandidate;
  id: string;
  onAccept: (candidate: MentionCandidate) => void;
  onMouseDown: (event: MouseEvent<HTMLButtonElement>) => void;
}) {
  const meta = mentionOptionMeta(candidate);
  const displayLabel = peopleFacingLabel(candidate.label);
  return (
    <button
      id={id}
      className={`composer-autocomplete-option ${active ? "is-active" : ""}`}
      key={candidate.key}
      type="button"
      role="option"
      aria-label={mentionOptionAriaLabel(candidate)}
      aria-selected={active ? "true" : "false"}
      data-mention-key={candidate.key}
      onMouseDown={onMouseDown}
      onClick={() => onAccept(candidate)}
    >
      <EntityAvatar
        avatar={candidate.avatar ?? null}
        className={`mention-option-avatar ${
          candidate.target.kind === "roomMention" ? "is-room-mention" : "is-user"
        }`}
        colorSeed={mentionTargetKey(candidate.target)}
        fallback={candidate.target.kind === "roomMention" ? "@" : initials(displayLabel)}
      />
      <span className="mention-option-main">
        <span className="mention-option-label" dir="auto">
          {displayLabel}
        </span>
        <span className="mention-option-meta" dir="auto" aria-hidden="true">
          {meta}
        </span>
      </span>
    </button>
  );
}

function mentionOptionMeta(candidate: MentionCandidate): string {
  switch (candidate.target.kind) {
    case "user":
      return candidate.target.user_id;
    case "room":
      return candidate.target.room_id;
    case "roomMention":
      return t("composer.mentionRoomNotificationDescription");
  }
}

function mentionOptionAriaLabel(candidate: MentionCandidate): string {
  const meta = mentionOptionMeta(candidate);
  const label = peopleFacingLabel(candidate.label);
  return meta ? `${label} ${meta}` : label;
}

function ThreadComposer({
  canEdit,
  document,
  draftKey,
  hasStagedUploads = false,
  isSending,
  mentionCandidates = [],
  mentionCandidatesLoading = false,
  notice = null,
  roomName = t("panel.thread"),
  resolveComposerKeyAction,
  onAttachFiles,
  onDocumentChange,
  onMentionQueryChange,
  onScheduleSend,
  onSend
}: {
  canEdit: boolean;
  document: ComposerDocument;
  draftKey: string;
  hasStagedUploads?: boolean;
  isSending: boolean;
  mentionCandidates?: MentionCandidate[];
  mentionCandidatesLoading?: boolean;
  notice?: string | null;
  roomName?: string;
  resolveComposerKeyAction: ResolveComposerKeyAction;
  onAttachFiles?: (files: File[]) => void | Promise<void>;
  onDocumentChange: (document: ComposerDocument) => void;
  onMentionQueryChange?: (query: string | null) => void;
  onScheduleSend?: (sendAtMs: number, document: ComposerDocument) => void | Promise<void>;
  onSend: (document: ComposerDocument) => void | Promise<void>;
}) {
  return (
    <Composer
      surface="thread"
      canEdit={canEdit}
      composerMode={{ kind: "plain" }}
      hasStagedUploads={hasStagedUploads}
      isSending={isSending}
      mentionCandidates={mentionCandidates}
      mentionCandidatesLoading={mentionCandidatesLoading}
      notice={notice}
      resolveComposerKeyAction={resolveComposerKeyAction}
      draftKey={draftKey}
      ariaLabel={t("timeline.threadComposer")}
      document={document}
      placeholder={t("timeline.threadPlaceholder")}
      roomName={roomName}
      onAttachFiles={onAttachFiles}
      onCancelReply={() => undefined}
      onDocumentChange={onDocumentChange}
      onMentionQueryChange={onMentionQueryChange}
      onScheduleSend={onScheduleSend}
      onSend={onSend}
    />
  );
}

export { ThreadComposer };

/** Shared mention popup used by normal, thread, and inline-edit composers. */
export function MentionAutocomplete({
  open,
  listboxId,
  activeIndex,
  candidates,
  loading,
  activeOptionId,
  onAccept,
  onMouseDown
}: {
  open: boolean;
  listboxId: string;
  activeIndex: number;
  candidates: MentionCandidate[];
  loading: boolean;
  activeOptionId?: string;
  onAccept: (candidate: MentionCandidate) => void;
  onMouseDown: (event: MouseEvent<HTMLButtonElement>) => void;
}) {
  if (!open) {
    return null;
  }
  return (
    <div
      id={listboxId}
      className="composer-autocomplete"
      role="listbox"
      aria-label={t("composer.mentionSuggestions")}
      aria-activedescendant={activeOptionId}
    >
      {mentionSections(candidates).map((section) => (
        <div className="composer-autocomplete-section" key={section.key} role="presentation">
          <div className="composer-autocomplete-section-heading">{section.label}</div>
          {section.candidates.map(({ candidate, index }) => (
            <MentionOption
              active={index === activeIndex}
              candidate={candidate}
              id={`${listboxId}-option-${index}`}
              key={candidate.key}
              onAccept={onAccept}
              onMouseDown={onMouseDown}
            />
          ))}
        </div>
      ))}
      {loading ? (
        <div className="composer-autocomplete-loading" role="status">
          {t("composer.mentionLoading")}
        </div>
      ) : null}
    </div>
  );
}
