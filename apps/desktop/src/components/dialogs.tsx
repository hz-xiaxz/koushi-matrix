// Dialog components extracted from App.tsx.
// Imports: React, lucide-react, i18n, domain types, uiShared.

import {
  type KeyboardEvent,
  useEffect,
  useState
} from "react";
import {
  Copy,
  FileText,
  Image as ImageIcon,
  X
} from "lucide-react";
import { type MessageId, t } from "../i18n/messages";
import type {
  CreateRoomVisibility,
  InviteScopeSelection,
  InviteWorkflowState,
  StagedUploadFormatChoice,
  StagedUploadItem,
  StagedUploadOutputSelection,
  StagedUploadResizeChoice
} from "../domain/types";
import {
  ICON_SIZE,
  formatUploadBytes,
  formatUploadDimensions,
  captionBody,
  type ImageUploadVariantKindPayload,
  type ImageCompressionPlan
} from "../app/uiShared";
import { ImeSafeForm, ImeTextField } from "./ImeTextControl";

async function writeClipboardText(value: string): Promise<void> {
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
  }
}

// ===== CreateEntityDialog =====

export interface CreateRoomDialogOptions {
  aliasLocalpart: string;
  encrypted: boolean;
  topic: string;
  visibility: CreateRoomVisibility;
}

export function CreateEntityDialog({
  activeSpaceName = null,
  isBusy,
  kind,
  roomOptions,
  value,
  onCancel,
  onRoomOptionsChange,
  onSubmit,
  onValueChange
}: {
  activeSpaceName?: string | null;
  isBusy: boolean;
  kind: "room" | "space";
  roomOptions?: CreateRoomDialogOptions;
  value: string;
  onCancel: () => void;
  onRoomOptionsChange?: (options: CreateRoomDialogOptions) => void;
  onSubmit: () => void;
  onValueChange: (value: string) => void;
}) {
  const isSpace = kind === "space";
  const effectiveRoomOptions =
    roomOptions ??
    ({
      aliasLocalpart: "",
      encrypted: true,
      topic: "",
      visibility: "private"
    } satisfies CreateRoomDialogOptions);
  const title = isSpace ? t("dialog.createSpaceTitle") : t("dialog.createRoomTitle");
  const inputLabel = isSpace ? t("dialog.spaceName") : t("dialog.roomName");
  const submitLabel = isSpace
    ? t("dialog.submitCreateSpace")
    : t("dialog.submitCreateRoom");
  const canSubmit =
    value.trim().length > 0 &&
    (isSpace ||
      effectiveRoomOptions.visibility === "private" ||
      effectiveRoomOptions.aliasLocalpart.trim().length > 0) &&
    !isBusy;

  function onDialogKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    }
  }

  function updateRoomOptions(patch: Partial<CreateRoomDialogOptions>) {
    const next = {
      ...effectiveRoomOptions,
      ...patch
    };
    if (next.visibility === "public") {
      next.encrypted = false;
    }
    onRoomOptionsChange?.(next);
  }

  return (
    <div
      className="dialog-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={onDialogKeyDown}
    >
      <ImeSafeForm
        className="dialog-box"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSubmit) {
            onSubmit();
          }
        }}
      >
        <div className="dialog-title">{title}</div>
        <ImeTextField
          className="dialog-input"
          type="text"
          autoFocus
          aria-label={inputLabel}
          placeholder={inputLabel}
          value={value}
          syncKey={`create-${kind}-name`}
          onChange={(event) => onValueChange(event.target.value)}
        />
        {!isSpace ? (
          <div className="create-room-options">
            <div className="create-room-visibility" role="radiogroup" aria-label={t("dialog.roomVisibility")}>
              <label className="create-room-option">
                <input
                  type="radio"
                  name="create-room-visibility"
                  checked={effectiveRoomOptions.visibility === "private"}
                  onChange={() =>
                    updateRoomOptions({
                      aliasLocalpart: "",
                      visibility: "private"
                    })
                  }
                />
                <span>{t("dialog.privateRoom")}</span>
              </label>
              <label className="create-room-option">
                <input
                  type="radio"
                  name="create-room-visibility"
                  checked={effectiveRoomOptions.visibility === "public"}
                  onChange={() =>
                    updateRoomOptions({
                      encrypted: false,
                      visibility: "public"
                    })
                  }
                />
                <span>{t("dialog.publicRoom")}</span>
              </label>
            </div>
            {activeSpaceName && effectiveRoomOptions.visibility === "private" ? (
              <div className="create-room-space-note">
                {t("dialog.standardRoomInSpace", { spaceName: activeSpaceName })}
              </div>
            ) : null}
            {effectiveRoomOptions.visibility === "private" ? (
              <label className="dialog-checkbox">
                <input
                  type="checkbox"
                  checked={effectiveRoomOptions.encrypted}
                  aria-label={t("dialog.encryptedRoom")}
                  onChange={(event) =>
                    updateRoomOptions({
                      encrypted: event.currentTarget.checked
                    })
                  }
                />
                <span>{t("dialog.encryptedRoom")}</span>
              </label>
            ) : null}
            <ImeTextField
              className="dialog-input"
              type="text"
              aria-label={t("dialog.roomTopic")}
              placeholder={t("dialog.roomTopic")}
              value={effectiveRoomOptions.topic}
              syncKey="create-room-topic"
              onChange={(event) =>
                updateRoomOptions({
                  topic: event.target.value
                })
              }
            />
            {effectiveRoomOptions.visibility === "public" ? (
              <ImeTextField
                className="dialog-input"
                type="text"
                aria-label={t("dialog.roomAddress")}
                placeholder={t("dialog.roomAddress")}
                value={effectiveRoomOptions.aliasLocalpart}
                syncKey="create-room-address"
                onChange={(event) =>
                  updateRoomOptions({
                    aliasLocalpart: event.target.value
                  })
                }
              />
            ) : null}
          </div>
        ) : null}
        <div className="dialog-actions">
          <button
            className="dialog-button"
            type="button"
            aria-label={t("dialog.cancelCreate")}
            onClick={onCancel}
          >
            {t("action.cancel")}
          </button>
          <button
            className="dialog-button is-primary"
            type="submit"
            aria-label={submitLabel}
            disabled={!canSubmit}
          >
            {isSpace ? t("action.createSpace") : t("action.createRoom")}
          </button>
        </div>
      </ImeSafeForm>
    </div>
  );
}

// ===== ImageCompressionDialog =====

export function ImageCompressionDialog({
  plan,
  onCancel,
  onChoose
}: {
  plan: ImageCompressionPlan;
  onCancel: () => void;
  onChoose: (choice: ImageUploadVariantKindPayload, saveDefault: boolean) => void;
}) {
  const [saveDefault, setSaveDefault] = useState(false);

  function onDialogKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    }
  }

  return (
    <div
      className="dialog-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={t("composer.imageCompressionTitle")}
      onKeyDown={onDialogKeyDown}
    >
      <div className="dialog-box image-compression-dialog">
        <div className="dialog-title">{t("composer.imageCompressionTitle")}</div>
        <div className="image-compression-preview">
          <img src={plan.compressed.previewUrl} alt={t("composer.imageCompressionPreviewAlt")} />
        </div>
        <div className="image-compression-options">
          <button
            className="image-compression-option"
            type="button"
            onClick={() => onChoose("Original", saveDefault)}
          >
            <span>{t("composer.imageCompressionOriginal")}</span>
            <strong>
              {formatUploadBytes(plan.original.byteCount)} · {formatUploadDimensions(plan.original.dimensions)}
            </strong>
          </button>
          <button
            className="image-compression-option is-preferred"
            type="button"
            autoFocus
            onClick={() => onChoose("Compressed", saveDefault)}
          >
            <span>{t("composer.imageCompressionCompressed")}</span>
            <strong>
              {formatUploadBytes(plan.compressed.byteCount)} · {formatUploadDimensions(plan.compressed.dimensions)}
            </strong>
          </button>
        </div>
        <label className="dialog-checkbox">
          <input
            type="checkbox"
            checked={saveDefault}
            onChange={(event) => setSaveDefault(event.currentTarget.checked)}
          />
          <span>{t("composer.imageCompressionSaveDefault")}</span>
        </label>
        <div className="dialog-actions">
          <button className="dialog-button" type="button" onClick={onCancel}>
            {t("dialog.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ===== DiagnosticDialog =====

export function DiagnosticDialog({
  report,
  onClose
}: {
  report: string;
  onClose: () => void;
}) {
  function onDialogKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  return (
    <div
      className="dialog-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={t("diagnostics.title")}
      onKeyDown={onDialogKeyDown}
    >
      <div className="dialog-box diagnostics-dialog">
        <div className="dialog-title-row">
          <div className="dialog-title">{t("diagnostics.title")}</div>
          <button
            className="icon-button"
            type="button"
            aria-label={t("action.close")}
            onClick={onClose}
          >
            <X size={ICON_SIZE.small} />
          </button>
        </div>
        <pre className="diagnostics-output">{report}</pre>
        <div className="dialog-actions">
          <button
            className="dialog-button is-primary"
            type="button"
            aria-label={t("diagnostics.copy")}
            onClick={() => {
              void writeClipboardText(report);
            }}
          >
            <Copy size={ICON_SIZE.small} />
            <span>{t("diagnostics.copy")}</span>
          </button>
        </div>
      </div>
    </div>
  );
}

// ===== UserIdDialog =====

export function UserIdDialog({
  inputLabel,
  isBusy,
  submitLabel,
  title,
  value,
  onCancel,
  onSubmit,
  onValueChange
}: {
  inputLabel: string;
  isBusy: boolean;
  submitLabel: string;
  title: string;
  value: string;
  onCancel: () => void;
  onSubmit: () => void;
  onValueChange: (value: string) => void;
}) {
  const canSubmit = value.trim().length > 0 && !isBusy;

  function onDialogKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    }
  }

  return (
    <div
      className="dialog-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={onDialogKeyDown}
    >
      <ImeSafeForm
        className="dialog-box"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSubmit) {
            onSubmit();
          }
        }}
      >
        <div className="dialog-title">{title}</div>
        <ImeTextField
          className="dialog-input"
          type="text"
          autoFocus
          aria-label={inputLabel}
          placeholder={inputLabel}
          spellCheck={false}
          value={value}
          syncKey={title}
          onChange={(event) => onValueChange(event.target.value)}
        />
        <div className="dialog-actions">
          <button
            className="dialog-button"
            type="button"
            aria-label={t("action.cancel")}
            onClick={onCancel}
          >
            {t("action.cancel")}
          </button>
          <button
            className="dialog-button is-primary"
            type="submit"
            aria-label={submitLabel}
            disabled={!canSubmit}
          >
            {submitLabel}
          </button>
        </div>
      </ImeSafeForm>
    </div>
  );
}

// ===== InviteTargetsDialog =====

export function InviteTargetsDialog({
  isBusy,
  query,
  scope,
  title,
  workflow,
  onCancel,
  onQueryChange,
  onRemoveTarget,
  onScopeChange,
  onSelectCandidate,
  onSubmit
}: {
  isBusy: boolean;
  query: string;
  scope: InviteScopeSelection;
  title: string;
  workflow: InviteWorkflowState;
  onCancel: () => void;
  onQueryChange: (value: string) => void;
  onRemoveTarget: (userId: string) => void;
  onScopeChange: (scope: InviteScopeSelection) => void;
  onSelectCandidate: (userId: string) => void;
  onSubmit: () => void;
}) {
  const isPending = workflow.operation.kind === "pending";
  const canSubmit = workflow.selected_targets.length > 0 && !isBusy && !isPending;

  function onDialogKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    }
  }

  return (
    <div
      className="dialog-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={onDialogKeyDown}
    >
      <ImeSafeForm
        className="dialog-box invite-target-dialog"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSubmit) {
            onSubmit();
          }
        }}
      >
        <div className="dialog-title">{title}</div>
        {workflow.operation.kind === "completed" && workflow.operation.notice ? (
          <div className="invite-target-notice" role="status">
            {workflow.operation.notice}
          </div>
        ) : null}
        <div className="invite-selected-targets" aria-label={t("dialog.inviteSelectedTargets")}>
          {workflow.selected_targets.map((target) => (
            <span className="invite-selected-target" key={target.user_id}>
              <span>{target.display_label}</span>
              <button
                type="button"
                aria-label={t("dialog.removeInviteTarget")}
                onClick={() => onRemoveTarget(target.user_id)}
              >
                <X size={ICON_SIZE.micro} aria-hidden="true" />
              </button>
            </span>
          ))}
        </div>
        <ImeTextField
          className="dialog-input"
          type="text"
          autoFocus
          aria-label={t("dialog.inviteSearch")}
          placeholder={t("dialog.inviteSearch")}
          spellCheck={false}
          value={query}
          syncKey={title}
          onChange={(event) => onQueryChange(event.target.value)}
        />
        <div className="invite-target-candidates" aria-label={t("dialog.inviteCandidates")}>
          {workflow.query.candidates.map((candidate) => (
            <button
              type="button"
              key={candidate.user_id}
              className="invite-target-candidate"
              disabled={candidate.status !== "selectable"}
              onClick={() => onSelectCandidate(candidate.user_id)}
            >
              <span>{candidate.display_label}</span>
              <span>{candidate.user_id}</span>
            </button>
          ))}
          {workflow.query.explicit_user_id ? (
            <button
              type="button"
              className="invite-target-candidate"
              disabled={workflow.query.explicit_user_id.status !== "selectable"}
              onClick={() => onSelectCandidate(workflow.query.explicit_user_id!.user_id)}
            >
              <span>{workflow.query.explicit_user_id.display_label}</span>
              <span>
                {workflow.query.explicit_user_id.status === "invalidMatrixId"
                  ? t("dialog.inviteInvalidMatrixId")
                  : workflow.query.explicit_user_id.user_id}
              </span>
            </button>
          ) : null}
        </div>
        {workflow.scope_plan ? (
          <div className="invite-scope-options" aria-label={t("dialog.inviteScope")}>
            {workflow.scope_plan.options.map((option) => {
              const checked = inviteScopeKey(option.scope) === inviteScopeKey(scope);
              return (
                <label className="invite-scope-option" key={inviteScopeKey(option.scope)}>
                  <input
                    type="radio"
                    name="invite-scope"
                    checked={checked}
                    onChange={() => onScopeChange(option.scope)}
                  />
                  <span>{option.label}</span>
                </label>
              );
            })}
          </div>
        ) : null}
        <div className="dialog-actions">
          <button className="dialog-button" type="button" aria-label={t("action.cancel")} onClick={onCancel}>
            {t("action.cancel")}
          </button>
          <button className="dialog-button is-primary" type="submit" disabled={!canSubmit}>
            {t("dialog.sendInvite")}
          </button>
        </div>
      </ImeSafeForm>
    </div>
  );
}

function inviteScopeKey(scope: InviteScopeSelection): string {
  return scope.kind === "roomOnly" ? "roomOnly" : `parent:${scope.space_id}`;
}

// ===== ReportReasonDialog =====

export function ReportReasonDialog({
  reason,
  title,
  onCancel,
  onReasonChange,
  onSubmit
}: {
  reason: string;
  title: string;
  onCancel: () => void;
  onReasonChange: (reason: string) => void;
  onSubmit: () => void;
}) {
  const canSubmit = reason.trim().length > 0;

  function onDialogKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    }
  }

  return (
    <div
      className="dialog-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={onDialogKeyDown}
    >
      <ImeSafeForm
        className="dialog-box"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSubmit) {
            onSubmit();
          }
        }}
      >
        <div className="dialog-title">{title}</div>
        <label className="dialog-input-label">
          <span>{t("dialog.reportReasonLabel")}</span>
          <ImeTextField
            className="dialog-input"
            type="text"
            autoFocus
            aria-label={t("dialog.reportReasonLabel")}
            placeholder={t("dialog.reportReasonPlaceholder")}
            value={reason}
            syncKey="report-reason"
            onChange={(event) => onReasonChange(event.target.value)}
          />
        </label>
        <div className="dialog-actions">
          <button
            className="dialog-button"
            type="button"
            aria-label={t("action.cancel")}
            onClick={onCancel}
          >
            {t("action.cancel")}
          </button>
          <button
            className="dialog-button is-primary"
            type="submit"
            aria-label={t("action.report")}
            disabled={!canSubmit}
          >
            {t("action.report")}
          </button>
        </div>
      </ImeSafeForm>
    </div>
  );
}

// ===== UploadStagingDialog =====

export function UploadStagingDialog({
  items,
  onClear,
  onUpdateCaption,
  onSelectOutput,
  onRetryPreparation,
  onUseOriginal,
  onSendAttachments,
  loadPreview
}: {
  items: StagedUploadItem[];
  onClear: () => void | Promise<void>;
  onUpdateCaption: (stagedId: string, caption: string) => void | Promise<void>;
  onSelectOutput: (
    stagedId: string,
    selection: StagedUploadOutputSelection
  ) => void | Promise<void>;
  onRetryPreparation: (stagedId: string) => void | Promise<void>;
  onUseOriginal: (stagedId: string) => void | Promise<void>;
  /**
   * Sends the staged attachments. This is deliberately separate from the
   * composer's send: it never touches the message draft, just as the composer's
   * send never touches these attachments.
   */
  onSendAttachments: () => void | Promise<void>;
  loadPreview: (stagedId: string, variantId: string) => Promise<number[]>;
}) {
  return (
    <section
      className="upload-staging-dialog"
      role="dialog"
      aria-label={t("upload.dialogTitle")}
    >
      <div className="upload-staging-header">
        <h2>{t("upload.dialogTitle")}</h2>
        <button className="icon-button" type="button" aria-label={t("upload.clear")} onClick={onClear}>
          <X size={ICON_SIZE.small} />
        </button>
      </div>
      <div className="upload-staging-list">
        {items.map((item) => (
          <article className="upload-staging-item" key={item.staged_id}>
            <div className="upload-staging-file">
              {item.kind.kind === "image" ? (
                <ImageIcon size={ICON_SIZE.control} aria-hidden="true" />
              ) : (
                <FileText size={ICON_SIZE.control} aria-hidden="true" />
              )}
              <span className="upload-staging-name" dir="auto">
                {item.filename || t("composer.attachmentFallback")}
              </span>
              <span className="upload-staging-meta">
                {formatUploadBytes(item.byte_count)}
              </span>
            </div>
            {item.kind.kind === "image" && item.preparation.kind === "ready" ? (
              <PreparedUploadPreview item={item} loadPreview={loadPreview} />
            ) : null}
            <label className="upload-staging-caption">
              <span>{t("upload.captionForFile", { filename: item.filename })}</span>
              <ImeTextField
                value={captionBody(item)}
                syncKey={item.staged_id}
                aria-label={t("upload.captionForFile", { filename: item.filename })}
                onChange={(event) => {
                  void onUpdateCaption(item.staged_id, event.currentTarget.value);
                }}
              />
            </label>
            {item.preparation.kind === "preparing" ? (
              <p className="upload-staging-status">{t("upload.preparing")}</p>
            ) : item.preparation.kind === "failed" ? (
              <div className="upload-staging-failure">
                <p className="upload-staging-status is-error">{t("upload.preparationFailed")}</p>
                <div className="upload-staging-failure-actions">
                  <button className="dialog-button" type="button" onClick={() => void onRetryPreparation(item.staged_id)}>
                    {t("upload.retryPreparation")}
                  </button>
                  {item.preparation.can_use_original ? (
                    <button className="dialog-button" type="button" onClick={() => void onUseOriginal(item.staged_id)}>
                      {t("upload.useOriginal")}
                    </button>
                  ) : null}
                </div>
              </div>
            ) : item.kind.kind === "image" ? (
              <UploadOutputToolbar
                item={item}
                preparation={item.preparation}
                onSelectOutput={onSelectOutput}
              />
            ) : null}
          </article>
        ))}
      </div>
      <div className="upload-staging-actions">
        <button
          className="dialog-button primary upload-staging-send"
          type="button"
          disabled={!uploadStagingItemsAreSendable(items)}
          onClick={() => void onSendAttachments()}
        >
          {t("upload.sendAttachments")}
        </button>
      </div>
    </section>
  );
}

/**
 * Attachments may be sent once every item has a prepared output and none is
 * still recompressing, so the bytes that upload are the ones the dialog shows.
 */
function uploadStagingItemsAreSendable(items: StagedUploadItem[]): boolean {
  return (
    items.length > 0 &&
    items.every(
      (item) => item.preparation.kind === "ready" && item.preparation.pending == null
    )
  );
}

/** Resize options, in the order the toolbar renders them. */
const UPLOAD_RESIZE_OPTIONS: ReadonlyArray<{
  value: StagedUploadResizeChoice;
  labelId: MessageId;
}> = [
  { value: "original", labelId: "upload.resizeOriginal" },
  { value: "half", labelId: "upload.resizeHalf" },
  { value: "quarter", labelId: "upload.resizeQuarter" },
  { value: "eighth", labelId: "upload.resizeEighth" }
];

/** Format options, in the order the toolbar renders them. */
const UPLOAD_FORMAT_OPTIONS: ReadonlyArray<{
  value: StagedUploadFormatChoice;
  labelId: MessageId;
}> = [
  { value: "keep", labelId: "upload.formatKeep" },
  { value: "webp", labelId: "upload.formatWebp" },
  { value: "jpeg", labelId: "upload.formatJpeg" },
  { value: "png", labelId: "upload.formatPng" }
];

/**
 * Two independent compact segmented controls plus one result summary.
 *
 * The pressed state comes from the Rust-owned selection, never from a local
 * click: a click only dispatches the chosen pair.
 */
function UploadOutputToolbar({
  item,
  preparation,
  onSelectOutput
}: {
  item: StagedUploadItem;
  preparation: Extract<StagedUploadItem["preparation"], { kind: "ready" }>;
  onSelectOutput: (
    stagedId: string,
    selection: StagedUploadOutputSelection
  ) => void | Promise<void>;
}) {
  const { selected } = preparation;
  const prepared = preparation.variants.find(
    (variant) =>
      variant.resize === selected.resize && variant.format_choice === selected.format
  );
  const recompressing = preparation.pending != null && prepared === undefined;
  return (
    <div className="upload-output-toolbar">
      <div
        className="upload-output-group"
        role="radiogroup"
        aria-label={t("upload.resizeChoice")}
      >
        <span className="upload-output-group-label">{t("upload.resizeChoice")}</span>
        {UPLOAD_RESIZE_OPTIONS.map((option) => (
          <button
            className="upload-output-option"
            type="button"
            key={option.value}
            role="radio"
            aria-checked={selected.resize === option.value}
            onClick={() =>
              void onSelectOutput(item.staged_id, {
                resize: option.value,
                format: selected.format
              })
            }
          >
            {t(option.labelId)}
          </button>
        ))}
      </div>
      <div
        className="upload-output-group"
        role="radiogroup"
        aria-label={t("upload.formatChoice")}
      >
        <span className="upload-output-group-label">{t("upload.formatChoice")}</span>
        {UPLOAD_FORMAT_OPTIONS.map((option) => (
          <button
            className="upload-output-option"
            type="button"
            key={option.value}
            role="radio"
            aria-checked={selected.format === option.value}
            onClick={() =>
              void onSelectOutput(item.staged_id, {
                resize: selected.resize,
                format: option.value
              })
            }
          >
            {t(option.labelId)}
          </button>
        ))}
      </div>
      <div
        className="upload-output-summary"
        role="status"
        aria-label={t("upload.outputSummary")}
        data-upload-output-state={recompressing ? "recompressing" : "ready"}
      >
        {recompressing ? (
          <span>{t("upload.recompressing")}</span>
        ) : prepared ? (
          <>
            <span>{formatPreparedDimensions(prepared.width, prepared.height)}</span>
            <span>{formatUploadBytes(prepared.byte_count)}</span>
            {prepared.savings_percent > 0 ? (
              <span>{t("upload.savings", { percent: prepared.savings_percent })}</span>
            ) : null}
          </>
        ) : (
          <span>{t("upload.preparing")}</span>
        )}
      </div>
    </div>
  );
}

function formatPreparedDimensions(width: number | null, height: number | null): string {
  return width === null || height === null
    ? "—"
    : formatUploadDimensions({ width, height });
}

function PreparedUploadPreview({
  item,
  loadPreview
}: {
  item: StagedUploadItem;
  loadPreview: (stagedId: string, variantId: string) => Promise<number[]>;
}) {
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  // The preview follows the Rust-owned selection: find the prepared output for
  // the selected pair. While a pair is `pending` there is none yet, so the
  // previously loaded preview stays on screen.
  const selectedVariantId =
    item.preparation.kind === "ready"
      ? item.preparation.variants.find(
          (variant) =>
            item.preparation.kind === "ready" &&
            variant.resize === item.preparation.selected.resize &&
            variant.format_choice === item.preparation.selected.format
        )?.variant_id ?? null
      : null;

  const recompressing =
    item.preparation.kind === "ready" &&
    item.preparation.pending != null &&
    selectedVariantId === null;

  useEffect(() => {
    let cancelled = false;
    let objectUrl: string | null = null;
    if (!selectedVariantId) {
      // Keep the last valid preview on screen: clearing it here would blank the
      // viewport while a new pair is still encoding, or after a failure.
      return;
    }
    void loadPreview(item.staged_id, selectedVariantId)
      .then((bytes) => {
        if (cancelled || bytes.length === 0) return;
        objectUrl = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: item.mime_type }));
        // Swap image and metadata together: the summary reads the same
        // Rust-owned prepared output this URL was built from.
        setPreviewUrl((previous) => {
          if (previous) URL.revokeObjectURL(previous);
          return objectUrl;
        });
        objectUrl = null;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [item.mime_type, item.staged_id, loadPreview, selectedVariantId]);

  // One fixed-height viewport that never collapses: recompression dims the
  // current preview instead of unmounting it.
  return (
    <div
      className="upload-preview-viewport"
      data-recompressing={recompressing ? "true" : undefined}
    >
      {previewUrl ? (
        <img className="upload-staging-preview" src={previewUrl} alt={t("upload.previewAlt")} />
      ) : (
        <div className="upload-staging-preview-placeholder" aria-label={t("upload.previewAlt")} />
      )}
      {recompressing ? (
        <span className="upload-preview-progress" role="presentation" />
      ) : null}
    </div>
  );
}
