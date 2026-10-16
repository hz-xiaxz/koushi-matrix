import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import {
  ArrowLeft,
  Bell,
  ChevronRight,
  Mail,
  MessageCircle,
  Plus,
  Send,
  Server,
  Trash2
} from "lucide-react";

import { openExternalHttpUrl } from "../../backend/linkMediaRuntime";
import { t, type MessageId } from "../../i18n/messages";
import { ImeSafeForm, ImeTextField, SecureImeTextField } from "../ImeTextControl";
import {
  accountNotificationsFailureMessage,
  isKeywordOperation
} from "./AccountNotificationsSections";
import { uiDateTimeFormat } from "../../domain/intlFormatCache";
import { roomDisplayLabel } from "../../domain/roomDisplayLabel";
import {
  DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES,
  DEFAULT_NOTIFICATION_SENDER_NAME,
  DEFAULT_RESEND_FROM_ADDRESS,
  MAX_NOTIFICATION_DELIVERY_SINKS,
  MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS,
  SERVERCHAN_SETUP_URL,
  clampNotificationDeliveryPrefixGraphemes,
  clampNotificationDeliverySeconds,
  createNotificationSinkId,
  newNotificationSink,
  notificationConsentRecipient,
  notificationContentConsented,
  notificationCredentialIdentity,
  notificationDeliveryOutcomeMessage,
  notificationDeliveryServiceLabel,
  notificationDeliverySinkFailed,
  notificationEffectiveContent,
  notificationSinkFieldsReady,
  notificationSinkSetup,
  notificationSmtpConfigFromPreset,
  notificationTargetsForLabel,
  notificationTextBudget,
  serverChanSendKeyProblem,
  type NotificationSinkKind,
  type NotificationSinkSetup,
  type ServerChanSendKeyProblem
} from "../../domain/notificationDelivery";
import type {
  AccountNotificationsOperationState,
  NotificationDeliveryContent,
  NotificationDeliveryPresetState,
  NotificationDeliverySettings,
  NotificationDeliverySinkState,
  NotificationDeliveryState,
  NotificationDeliveryTarget,
  NotificationKeywordRule,
  NotificationSinkSettings,
  RoomSummary,
  SmtpTls,
  SpaceSummary
} from "../../domain/types";

/**
 * Typed commands of the device-local notification delivery section (#1427, #1460, #1468).
 *
 * `save` is the detail screen's single Save: the non-secret destination, the secret the
 * user typed (or `null` to keep the stored one), and whether the plaintext warning was
 * accepted. Rust validates, writes, stores the secret, and mints the consent; a refusal
 * rejects. `update` carries the whole delivery block and is used only to remove a
 * destination, after `clearSecret` released its credential. None of them returns the
 * value that was written.
 */
export interface NotificationDeliveryActions {
  update: (delivery: NotificationDeliverySettings) => void;
  save: (
    sink: NotificationSinkSettings,
    secret: string | null,
    acceptContentConsent: boolean
  ) => Promise<void>;
  clearSecret: (sinkId: string) => Promise<void>;
  test: (sinkId: string) => Promise<void>;
}

export const noopNotificationDeliveryActions: NotificationDeliveryActions = {
  update: () => undefined,
  save: async () => undefined,
  clearSecret: async () => undefined,
  test: async () => undefined
};

/**
 * The write half of the keyword editor (#1427). The rules themselves are the
 * account's server state, so they are read from `AccountNotificationsSnapshot`
 * and dispatched through the account notification action surface; this device
 * section only borrows the switch that delivers for them.
 */
export interface NotificationKeywordActions {
  add: (pattern: string) => void;
  remove: (ruleId: string) => void;
}

export const noopNotificationKeywordActions: NotificationKeywordActions = {
  add: () => undefined,
  remove: () => undefined
};

const SINK_KINDS: readonly NotificationSinkKind[] = ["resend", "smtp", "serverChan"];

function sinkKindLabel(kind: NotificationSinkKind): string {
  switch (kind) {
    case "resend":
      return t("settings.notificationDeliverySinkKindResend");
    case "smtp":
      return t("settings.notificationDeliverySinkKindSmtp");
    case "serverChan":
      return t("settings.notificationDeliverySinkKindServerChan");
  }
}

function sinkKindDescription(kind: NotificationSinkKind): string {
  switch (kind) {
    case "resend":
      return t("settings.notificationDeliveryPickResend");
    case "smtp":
      return t("settings.notificationDeliveryPickSmtp");
    case "serverChan":
      return t("settings.notificationDeliveryPickServerChan");
  }
}

function SinkKindIcon({ kind }: { kind: NotificationSinkKind }) {
  switch (kind) {
    case "resend":
      return <Mail size={18} aria-hidden="true" />;
    case "smtp":
      return <Server size={18} aria-hidden="true" />;
    case "serverChan":
      return <MessageCircle size={18} aria-hidden="true" />;
  }
}

function setupMessage(setup: NotificationSinkSetup): MessageId {
  switch (setup) {
    case "configured":
      return "settings.notificationDeliveryStatusConfigured";
    case "needsCredential":
      return "settings.notificationDeliveryStatusNeedsCredential";
    case "needsTargets":
      return "settings.notificationDeliveryStatusNeedsTargets";
    case "incomplete":
      return "settings.notificationDeliveryStatusIncomplete";
  }
}

/**
 * Which screen of the section is open (#1468). The detail screen edits `draft`, an
 * ephemeral copy that nothing writes until Save; `null` means it follows Rust's copy.
 * `added` stands in for a destination this screen just saved until the snapshot that
 * carries it arrives.
 */
type DeliveryView =
  | { kind: "list" }
  | { kind: "pick" }
  | {
      kind: "detail";
      sinkId: string;
      draft: NotificationSinkSettings | null;
      added: NotificationSinkSettings | null;
    };

/**
 * Settings -> Notifications, "Sent from this device" (#1460, #1468).
 *
 * A list of destinations, each opening a detail screen that owns its own connection,
 * targets, content and timing. Everything shown is Rust state: `delivery` is the settings
 * block, `state` is the app-scoped runtime half (whether a credential exists, and the last
 * outcome per sink), and `rooms`/`spaces` supply the labels the watch list renders. The
 * detail screen's edits stay a local draft until its single Save. This component computes
 * no dwell, starts no timer, assembles no message, builds no consent, and never reads a
 * secret back.
 */
export function NotificationDeliverySection({
  delivery,
  state,
  rooms = [],
  spaces = [],
  actions,
  keywordRules,
  keywordOperation,
  keywordActions = noopNotificationKeywordActions
}: {
  delivery: NotificationDeliverySettings;
  state: NotificationDeliveryState;
  rooms?: RoomSummary[];
  spaces?: SpaceSummary[];
  actions: NotificationDeliveryActions;
  /**
   * #1427: the account's own `content` rules and the account notification
   * operation state they settle through. Undefined means this surface has no
   * account (the app-scoped settings dialog), so the editor is not rendered.
   */
  keywordRules?: readonly NotificationKeywordRule[];
  keywordOperation?: AccountNotificationsOperationState;
  keywordActions?: NotificationKeywordActions;
}) {
  const [view, setView] = useState<DeliveryView>({ kind: "list" });
  const [testingSinkId, setTestingSinkId] = useState<string | null>(null);

  const sinkStates = useMemo(() => {
    const map = new Map<string, NotificationDeliverySinkState>();
    for (const sinkState of state.sinks) {
      map.set(sinkState.sink_id, sinkState);
    }
    return map;
  }, [state.sinks]);

  const atLimit = delivery.sinks.length >= MAX_NOTIFICATION_DELIVERY_SINKS;

  async function removeSink(sinkId: string) {
    // Removing the sink also releases the credential it was keyed by; leaving it behind
    // would keep an orphaned secret in the OS store. The credential goes first, and a
    // failed delete keeps the sink so the user can retry instead of losing its handle.
    try {
      await actions.clearSecret(sinkId);
    } catch {
      return;
    }
    actions.update({ ...delivery, sinks: delivery.sinks.filter((sink) => sink.id !== sinkId) });
    setView({ kind: "list" });
  }

  async function runTest(sinkId: string) {
    setTestingSinkId(sinkId);
    try {
      await actions.test(sinkId);
    } catch {
      // The outcome block reports what Rust recorded; a refused admission changes nothing.
    } finally {
      setTestingSinkId(null);
    }
  }

  let body;
  if (view.kind === "pick") {
    body = (
      <SinkKindPicker
        onPick={(kind) =>
          setView({
            kind: "detail",
            sinkId: "",
            draft: newNotificationSink(createNotificationSinkId(), kind),
            added: null
          })
        }
        onCancel={() => setView({ kind: "list" })}
      />
    );
  } else if (view.kind === "detail") {
    const sinkId = view.draft?.id ?? view.sinkId;
    const saved = delivery.sinks.find((candidate) => candidate.id === sinkId) ?? view.added;
    const sink = view.draft ?? saved;
    if (!sink) {
      // Rust dropped the destination (another window removed it, or the patch was refused).
      body = renderSinkList();
    } else {
      body = (
        <NotificationSinkDetail
          key={sink.id}
          sink={sink}
          saved={saved}
          sinkState={saved ? sinkStates.get(sink.id) ?? null : null}
          presets={state.presets}
          rooms={rooms}
          spaces={spaces}
          testing={testingSinkId === sink.id}
          keywordEditor={
            keywordRules !== undefined && keywordOperation !== undefined ? (
              <NotificationKeywordEditor
                rules={keywordRules}
                operation={keywordOperation}
                actions={keywordActions}
              />
            ) : null
          }
          onChange={(next) => setView({ kind: "detail", sinkId: next.id, draft: next, added: view.added })}
          onSave={async (next, secret, acceptContentConsent) => {
            await actions.save(next, secret, acceptContentConsent);
            setView({ kind: "detail", sinkId: next.id, draft: null, added: next });
          }}
          onDiscard={() =>
            saved
              ? setView({ kind: "detail", sinkId: sink.id, draft: null, added: view.added })
              : setView({ kind: "list" })
          }
          onBack={() => setView({ kind: "list" })}
          onRemove={() => void removeSink(sink.id)}
          onTest={() => runTest(sink.id)}
        />
      );
    }
  } else {
    body = renderSinkList();
  }

  function renderSinkList() {
    return (
      <>
        {delivery.sinks.length === 0 ? (
          <p className="settings-status-text" data-testid="notification-delivery-sinks-empty">
            {t("settings.notificationDeliverySinkEmpty")}
          </p>
        ) : (
          <ul className="notification-delivery-list" data-testid="notification-delivery-sinks">
            {delivery.sinks.map((sink) => (
              <li key={sink.id}>
                <SinkRow
                  sink={sink}
                  sinkState={sinkStates.get(sink.id) ?? null}
                  presets={state.presets}
                  onOpen={() => setView({ kind: "detail", sinkId: sink.id, draft: null, added: null })}
                />
              </li>
            ))}
          </ul>
        )}
        <div className="session-actions">
          <button
            className="trust-action-button secondary"
            type="button"
            data-testid="notification-delivery-add"
            disabled={atLimit}
            onClick={() => setView({ kind: "pick" })}
          >
            <Plus size={14} aria-hidden="true" />
            <span>{t("settings.notificationDeliveryAddSink")}</span>
          </button>
        </div>
        {atLimit ? (
          <p className="profile-settings-hint" data-testid="notification-delivery-sinks-limit">
            {t("settings.notificationDeliverySinkLimit", { count: MAX_NOTIFICATION_DELIVERY_SINKS })}
          </p>
        ) : null}
      </>
    );
  }

  return (
    <section
      className="settings-section notification-delivery"
      aria-label={t("settings.notificationDelivery")}
      data-testid="notification-delivery"
    >
      <div className="settings-section-heading">
        <div>
          <h3>{t("settings.notificationDelivery")}</h3>
          <p>{t("settings.notificationDeliveryDescription")}</p>
        </div>
      </div>
      <p className="profile-settings-hint" data-testid="notification-delivery-running-note">
        {t("settings.notificationDeliveryRunningNote")}
      </p>
      {body}
    </section>
  );
}

/** One destination in the list: what it is, where it goes, its setup, and a summary. */
function SinkRow({
  sink,
  sinkState,
  presets,
  onOpen
}: {
  sink: NotificationSinkSettings;
  sinkState: NotificationDeliverySinkState | null;
  presets: readonly NotificationDeliveryPresetState[];
  onOpen: () => void;
}) {
  const setup = notificationSinkSetup(sink, sinkState);
  const failed = notificationDeliverySinkFailed(sinkState?.last_outcome ?? null);
  const service = notificationDeliveryServiceLabel(sink, presets);
  const configured = setup === "configured";
  const status = configured
    ? t("settings.notificationDeliveryConfigured")
    : t("settings.notificationDeliveryNotConfigured");
  const chips = sinkSummaryChips(sink);
  return (
    <button
      className="notification-delivery-row"
      type="button"
      data-testid="notification-delivery-sink"
      data-sink-id={sink.id}
      data-setup={setup}
      data-state={failed ? "failed" : "ok"}
      aria-label={`${service}: ${status}`}
      onClick={onOpen}
    >
      <span className="notification-delivery-row-icon">
        <SinkKindIcon kind={sink.config.kind} />
      </span>
      <span className="notification-delivery-row-main">
        <span className="notification-delivery-row-title">
          <span dir="auto">{service}</span>
          <span
            className="notification-delivery-pill"
            data-configured={configured ? "true" : "false"}
            data-testid="notification-delivery-sink-status"
          >
            {status}
          </span>
        </span>
        {sink.config.kind !== "serverChan" && sink.destination ? (
          <span className="notification-delivery-row-destination" dir="auto">
            {sink.destination}
          </span>
        ) : null}
        {failed ? (
          <span className="notification-delivery-row-failed" data-testid="notification-delivery-sink-failed">
            {t("settings.notificationDeliverySinkFailed")}
          </span>
        ) : null}
        <span
          className="notification-delivery-chips"
          aria-label={t("settings.notificationDeliveryChipsLabel")}
          data-testid="notification-delivery-sink-chips"
        >
          {chips.map((chip) => (
            <span className="notification-delivery-chip" key={chip.key} data-chip={chip.key}>
              {chip.label}
            </span>
          ))}
        </span>
      </span>
      <ChevronRight size={16} aria-hidden="true" />
    </button>
  );
}

function sinkSummaryChips(sink: NotificationSinkSettings): { key: string; label: string }[] {
  const targets = sink.targets;
  const parts: string[] = [];
  if (targets.direct_messages) parts.push(t("settings.notificationDeliveryChipDirectMessages"));
  if (targets.mentions) parts.push(t("settings.notificationDeliveryChipMentions"));
  if (targets.keywords) parts.push(t("settings.notificationDeliveryChipKeywords"));
  if (targets.watched.length > 0) {
    parts.push(t("settings.notificationDeliveryChipWatched", { count: targets.watched.length }));
  }
  const content = notificationEffectiveContent(sink);
  return [
    {
      key: "targets",
      label: parts.length > 0 ? parts.join(" · ") : t("settings.notificationDeliveryChipNoTargets")
    },
    {
      key: "content",
      label:
        content.kind === "prefix"
          ? t("settings.notificationDeliveryChipPrefix", { count: content.graphemes })
          : content.kind === "full"
            ? t("settings.notificationDeliveryChipFull")
            : t("settings.notificationDeliveryChipNamesOnly")
    },
    {
      key: "timing",
      label: t("settings.notificationDeliveryChipDwell", {
        minutes: Math.round((sink.timing.dwell_secs / 60) * 10) / 10
      })
    }
  ];
}

/** The add button's sheet: one card per kind, each leading to a new destination's detail. */
function SinkKindPicker({
  onPick,
  onCancel
}: {
  onPick: (kind: NotificationSinkKind) => void;
  onCancel: () => void;
}) {
  return (
    <section
      className="notification-delivery-picker"
      aria-label={t("settings.notificationDeliveryPickTitle")}
      data-testid="notification-delivery-pick"
    >
      <h4 className="settings-subheading">{t("settings.notificationDeliveryPickTitle")}</h4>
      <ul className="notification-delivery-picker-list">
        {SINK_KINDS.map((kind) => (
          <li key={kind}>
            <button
              className="notification-delivery-row notification-delivery-pick-option"
              type="button"
              data-testid="notification-delivery-pick-kind"
              data-kind={kind}
              onClick={() => onPick(kind)}
            >
              <span className="notification-delivery-row-icon">
                <SinkKindIcon kind={kind} />
              </span>
              <span className="notification-delivery-row-main">
                <span className="notification-delivery-row-title">{sinkKindLabel(kind)}</span>
                <span className="notification-delivery-row-destination">{sinkKindDescription(kind)}</span>
              </span>
              <ChevronRight size={16} aria-hidden="true" />
            </button>
          </li>
        ))}
      </ul>
      <div className="session-actions">
        <button
          className="trust-action-button secondary"
          type="button"
          data-testid="notification-delivery-pick-cancel"
          onClick={onCancel}
        >
          {t("action.cancel")}
        </button>
      </div>
    </section>
  );
}

function trimSinkFields(sink: NotificationSinkSettings): NotificationSinkSettings {
  const config = sink.config;
  return {
    ...sink,
    destination: sink.destination.trim(),
    from_address: sink.from_address.trim(),
    sender_name: sink.sender_name.trim(),
    config:
      config.kind === "smtp"
        ? { ...config, host: config.host.trim(), username: config.username.trim() }
        : { ...config, endpoint: config.endpoint.trim() }
  };
}

/** Structural equality of two JSON-shaped DTO values, ignoring key order. */
function sameValue(left: unknown, right: unknown): boolean {
  if (left === right) {
    return true;
  }
  if (typeof left !== "object" || typeof right !== "object" || left === null || right === null) {
    return false;
  }
  if (Array.isArray(left) !== Array.isArray(right)) {
    return false;
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const keys = new Set([...Object.keys(leftRecord), ...Object.keys(rightRecord)]);
  for (const key of keys) {
    if (!sameValue(leftRecord[key] ?? null, rightRecord[key] ?? null)) {
      return false;
    }
  }
  return true;
}

/**
 * One text field of the draft. Every keystroke updates the draft; nothing is written until
 * Save. The sync key changes on Save and on Discard, so the field shows Rust's copy again.
 */
function SinkTextField({
  label,
  value,
  field,
  syncScope,
  testId,
  type = "text",
  placeholder,
  required = false,
  onChange
}: {
  label: string;
  value: string;
  field: string;
  syncScope: string;
  testId: string;
  type?: "text" | "email" | "url";
  placeholder?: string;
  required?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label className="profile-settings-field">
      <span>{label}</span>
      <ImeTextField
        type={type}
        value={value}
        autoComplete="off"
        placeholder={placeholder}
        aria-required={required ? true : undefined}
        syncKey={`sink-${field}:${syncScope}`}
        data-testid={testId}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
    </label>
  );
}

/**
 * A bounded number of the draft, committed on blur so a half-typed value is not clamped
 * while the user types. The key names the committed value, so the clamp is shown.
 */
function NumberField({
  label,
  value,
  field,
  syncScope,
  testId,
  maxLength,
  clamp,
  onChange
}: {
  label: string;
  value: number;
  field: string;
  syncScope: string;
  testId: string;
  maxLength: number;
  clamp: (value: number) => number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="profile-settings-field">
      <span>{label}</span>
      <ImeTextField
        type="text"
        inputMode="numeric"
        maxLength={maxLength}
        value={String(value)}
        aria-label={label}
        syncKey={`sink-${field}:${syncScope}:${value}`}
        data-testid={testId}
        onBlur={(event) => {
          const next = clamp(Number(event.currentTarget.value));
          if (next !== value) {
            onChange(next);
          } else if (event.currentTarget.value !== String(value)) {
            event.currentTarget.value = String(value);
          }
        }}
      />
    </label>
  );
}

function SecondsField({
  label,
  hint,
  value,
  field,
  syncScope,
  testId,
  onChange
}: {
  label: string;
  hint: string;
  value: number;
  field: string;
  syncScope: string;
  testId: string;
  onChange: (value: number) => void;
}) {
  return (
    <>
      <NumberField
        label={label}
        value={value}
        field={field}
        syncScope={syncScope}
        testId={testId}
        maxLength={6}
        clamp={clampNotificationDeliverySeconds}
        onChange={onChange}
      />
      <p className="profile-settings-hint">
        {hint}{" "}
        {t("settings.notificationDeliverySecondsAsMinutes", {
          minutes: Math.round((value / 60) * 10) / 10
        })}
      </p>
    </>
  );
}

function formatAttemptTime(atMs: number): string {
  return uiDateTimeFormat({ dateStyle: "medium", timeStyle: "short" }).format(
    new Date(atMs)
  );
}

function secretLabel(kind: NotificationSinkKind): string {
  switch (kind) {
    case "resend":
      return t("settings.notificationDeliverySecretResend");
    case "smtp":
      return t("settings.notificationDeliverySinkSecret");
    case "serverChan":
      return t("settings.notificationDeliverySecretServerChan");
  }
}

function sendKeyProblemMessage(problem: ServerChanSendKeyProblem): MessageId {
  return problem === "serverChan3Key"
    ? "settings.notificationDeliveryServerChanKeySctp"
    : "settings.notificationDeliveryServerChanKeyMalformed";
}

type SaveProblem = "fields" | "secret" | "secretReenter" | "consent";

type SaveState = { kind: "idle" } | { kind: "saving" } | { kind: "failed" };

/**
 * The detail screen of one destination (#1468): Connection, Targets, Content and Timing,
 * then Status with Test and Remove once it is saved. `sink` is the draft (or Rust's copy
 * when nothing is edited); `saved` is Rust's copy, `null` for a destination being added.
 *
 * The secret is DOM-owned: the field is never given a value, its text is read only when
 * Save builds the command, and the field is cleared right then. React keeps only whether
 * the field holds anything and, for ServerChan, which shape problem it has.
 */
function NotificationSinkDetail({
  sink,
  saved,
  sinkState,
  presets,
  rooms,
  spaces,
  testing,
  keywordEditor,
  onChange,
  onSave,
  onDiscard,
  onBack,
  onRemove,
  onTest
}: {
  sink: NotificationSinkSettings;
  saved: NotificationSinkSettings | null;
  sinkState: NotificationDeliverySinkState | null;
  presets: readonly NotificationDeliveryPresetState[];
  rooms: readonly RoomSummary[];
  spaces: readonly SpaceSummary[];
  testing: boolean;
  keywordEditor: ReactNode;
  onChange: (sink: NotificationSinkSettings) => void;
  onSave: (sink: NotificationSinkSettings, secret: string | null, acceptContentConsent: boolean) => Promise<void>;
  onDiscard: () => void;
  onBack: () => void;
  onRemove: () => void;
  onTest: () => Promise<void>;
}) {
  const secretRef = useRef<HTMLInputElement | null>(null);
  const [secretFilled, setSecretFilled] = useState(false);
  const [secretProblem, setSecretProblem] = useState<ServerChanSendKeyProblem | null>(null);
  const [consentChecked, setConsentChecked] = useState(false);
  const [problem, setProblem] = useState<SaveProblem | null>(null);
  const [saveState, setSaveState] = useState<SaveState>({ kind: "idle" });
  // Bumped on Save and Discard so every field shows Rust's copy again.
  const [epoch, setEpoch] = useState(0);
  const syncScope = `${sink.id}:${epoch}`;

  // Held as a local const so the discriminated union stays narrowed inside the handlers.
  const config = sink.config;
  const isNew = saved === null;
  const serverChan = config.kind === "serverChan";
  const service = notificationDeliveryServiceLabel(sink, presets);
  const label = secretLabel(config.kind);
  const trimmed = trimSinkFields(sink);
  const identityChanged =
    saved !== null && notificationCredentialIdentity(trimmed) !== notificationCredentialIdentity(saved);
  const secretConfigured = Boolean(sinkState?.secret_configured) && !identityChanged;
  // A text mode needs a consent naming this exact kind, endpoint and recipient. A stored one
  // that still matches keeps covering it, unless a new ServerChan SendKey is entered: that
  // is a new recipient revision, so the warning is shown again.
  const textMode = sink.content.kind !== "namesOnly";
  const consentCovered = notificationContentConsented(trimmed) && !(serverChan && secretFilled);
  const needsConsent = textMode && !consentCovered;
  // ServerChan's recipient is the revision of the saved SendKey; with neither a saved key
  // nor a newly entered one there is nothing a consent could name.
  const serverChanHasKey = secretFilled || notificationConsentRecipient(trimmed) !== "";
  const secretRequired =
    isNew || identityChanged || (serverChan && needsConsent && !serverChanHasKey);
  const dirty =
    isNew ||
    !sameValue(trimmed, saved) ||
    secretFilled ||
    (needsConsent && consentChecked);
  const saving = saveState.kind === "saving";
  const setup = notificationSinkSetup(sink, sinkState);

  function change(next: NotificationSinkSettings) {
    setProblem(null);
    if (saveState.kind === "failed") {
      setSaveState({ kind: "idle" });
    }
    onChange(next);
  }

  function clearSecretField() {
    const field = secretRef.current;
    if (field) {
      field.value = "";
    }
    setSecretFilled(false);
    setSecretProblem(null);
  }

  async function save() {
    if (saving) {
      return;
    }
    if (!notificationSinkFieldsReady(trimmed)) {
      setProblem("fields");
      return;
    }
    const field = secretRef.current;
    const typed = field ? (serverChan ? field.value.trim() : field.value) : "";
    if (serverChan && typed) {
      const shape = serverChanSendKeyProblem(typed);
      if (shape !== null) {
        setSecretProblem(shape);
        field?.focus();
        return;
      }
    }
    if (secretRequired && !typed) {
      setProblem(identityChanged ? "secretReenter" : "secret");
      field?.focus();
      return;
    }
    if (needsConsent && !consentChecked) {
      setProblem("consent");
      return;
    }
    const acceptContentConsent = needsConsent && consentChecked;
    // The secret leaves the DOM exactly once, into the command; the field is emptied now
    // so a failed save cannot leave it behind.
    clearSecretField();
    setProblem(null);
    setSaveState({ kind: "saving" });
    try {
      await onSave(trimmed, typed === "" ? null : typed, acceptContentConsent);
    } catch {
      setSaveState({ kind: "failed" });
      return;
    }
    setSaveState({ kind: "idle" });
    setConsentChecked(false);
    setEpoch((value) => value + 1);
  }

  function discard() {
    clearSecretField();
    setConsentChecked(false);
    setProblem(null);
    setSaveState({ kind: "idle" });
    setEpoch((value) => value + 1);
    onDiscard();
  }

  const problemMessage =
    problem === "fields"
      ? t("settings.notificationDeliveryFieldsRequired")
      : problem === "secret"
        ? t("settings.notificationDeliverySecretRequired", { label })
        : problem === "secretReenter"
          ? t("settings.notificationDeliverySecretReenter", { label })
          : problem === "consent"
            ? t("settings.notificationDeliveryConsentRequired")
            : null;

  return (
    <article
      className="notification-delivery-detail"
      data-testid="notification-delivery-detail"
      data-sink-id={sink.id}
      data-kind={config.kind}
      data-draft={isNew ? "true" : "false"}
      data-dirty={dirty ? "true" : "false"}
    >
      <nav className="notification-delivery-breadcrumb" aria-label={t("settings.notificationDelivery")}>
        <button
          className="profile-settings-action"
          type="button"
          data-testid="notification-delivery-back"
          onClick={onBack}
        >
          <ArrowLeft size={14} aria-hidden="true" />
          <span>{t("settings.notificationDeliveryBack")}</span>
        </button>
        <ChevronRight size={14} aria-hidden="true" />
        <span aria-current="page" dir="auto">
          {isNew ? t("settings.notificationDeliveryNewDestination") : service}
        </span>
      </nav>
      <h4 className="notification-delivery-detail-title" dir="auto">
        <SinkKindIcon kind={config.kind} />
        <span>{service}</span>
      </h4>

      <section
        className="notification-delivery-group"
        aria-label={t("settings.notificationDeliveryCredentials")}
        data-testid="notification-delivery-credentials"
      >
        <h5 className="settings-subheading">{t("settings.notificationDeliveryCredentials")}</h5>
        {config.kind === "serverChan" ? <ServerChanSetup /> : null}
        {config.kind === "smtp" ? (
          <>
            <label className="profile-settings-field">
              <span>{t("settings.notificationDeliverySinkPreset")}</span>
              <select
                data-testid="notification-delivery-sink-preset"
                value={config.preset ?? ""}
                onChange={(event) => {
                  const preset = presets.find((candidate) => candidate.id === event.currentTarget.value);
                  change({
                    ...sink,
                    config: preset
                      ? notificationSmtpConfigFromPreset(preset, sink.from_address)
                      : { ...config, preset: null }
                  });
                }}
              >
                <option value="">{t("settings.notificationDeliverySinkPresetCustom")}</option>
                {presets.map((preset) => (
                  <option key={preset.id} value={preset.id}>
                    {preset.label}
                  </option>
                ))}
              </select>
            </label>
            <p className="profile-settings-hint">{t("settings.notificationDeliverySinkPresetHint")}</p>
            <SinkTextField
              label={t("settings.notificationDeliverySinkFrom")}
              type="email"
              field="from"
              value={sink.from_address}
              syncScope={syncScope}
              required
              testId="notification-delivery-sink-from"
              onChange={(from_address) => change({ ...sink, from_address })}
            />
            <SinkTextField
              label={t("settings.notificationDeliverySinkName")}
              field="name"
              value={sink.sender_name}
              syncScope={syncScope}
              placeholder={DEFAULT_NOTIFICATION_SENDER_NAME}
              testId="notification-delivery-sink-name"
              onChange={(sender_name) => change({ ...sink, sender_name })}
            />
          </>
        ) : null}
        {config.kind !== "serverChan" ? (
          <SinkTextField
            label={t("settings.notificationDeliverySinkTo")}
            type="email"
            field="to"
            value={sink.destination}
            syncScope={syncScope}
            required
            testId="notification-delivery-sink-to"
            onChange={(destination) => change({ ...sink, destination })}
          />
        ) : null}
        {config.kind === "smtp" ? (
          <>
            <SinkTextField
              label={t("settings.notificationDeliverySinkHost")}
              field="host"
              value={config.host}
              syncScope={syncScope}
              required
              testId="notification-delivery-sink-host"
              onChange={(host) => change({ ...sink, config: { ...config, host } })}
            />
            <NumberField
              label={t("settings.notificationDeliverySinkPort")}
              value={config.port}
              field="port"
              syncScope={syncScope}
              testId="notification-delivery-sink-port"
              maxLength={5}
              clamp={smtpPort}
              onChange={(port) => change({ ...sink, config: { ...config, port } })}
            />
            <label className="profile-settings-field">
              <span>{t("settings.notificationDeliverySinkTls")}</span>
              <select
                data-testid="notification-delivery-sink-tls"
                value={config.tls}
                onChange={(event) =>
                  change({ ...sink, config: { ...config, tls: event.currentTarget.value as SmtpTls } })
                }
              >
                <option value="implicit">{t("settings.notificationDeliverySinkTlsImplicit")}</option>
                <option value="startTls">{t("settings.notificationDeliverySinkTlsStartTls")}</option>
              </select>
            </label>
            <SinkTextField
              label={t("settings.notificationDeliverySinkUsername")}
              field="username"
              value={config.username}
              syncScope={syncScope}
              testId="notification-delivery-sink-username"
              placeholder={
                selectedPresetIsFullAddress(presets, config.preset ?? null)
                  ? t("settings.notificationDeliverySinkUsernameFullAddress")
                  : undefined
              }
              onChange={(username) => change({ ...sink, config: { ...config, username } })}
            />
            <SecretHint presets={presets} presetId={config.preset ?? null} />
          </>
        ) : null}

        <label className="profile-settings-field">
          <span>{label}</span>
          <SecureImeTextField
            ref={secretRef}
            autoComplete="off"
            syncKey={`sink-secret:${syncScope}`}
            data-testid="notification-delivery-sink-secret"
            aria-label={label}
            aria-required={secretRequired ? true : undefined}
            aria-invalid={secretProblem || problem === "secret" || problem === "secretReenter" ? true : undefined}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setSecretFilled(value !== "");
              setProblem(null);
              setSecretProblem(serverChan && value ? serverChanSendKeyProblem(value) : null);
            }}
          />
        </label>
        {secretProblem ? (
          <p
            className="notification-delivery-error"
            role="alert"
            data-testid="notification-delivery-sink-secret-problem"
          >
            {t(sendKeyProblemMessage(secretProblem))}
          </p>
        ) : null}
        {isNew ? null : (
          <p className="profile-settings-hint" data-testid="notification-delivery-sink-secret-state">
            {identityChanged
              ? t("settings.notificationDeliverySecretReenter", { label })
              : secretConfigured
                ? t("settings.notificationDeliverySinkSecretSet")
                : t("settings.notificationDeliverySinkSecretMissing")}
          </p>
        )}
        {sinkState?.secret_retired && !sinkState.secret_configured ? (
          // Rust says when it deleted a stored credential because the sink stopped being the
          // server or account it was entered for, so the empty field is explained rather than
          // read as data the user lost.
          <p className="profile-settings-hint" data-testid="notification-delivery-sink-secret-retired">
            {t("settings.notificationDeliverySinkSecretRetired")}
          </p>
        ) : null}

        {config.kind !== "smtp" ? (
          <details className="settings-advanced" data-testid="notification-delivery-advanced">
            <summary>{t("settings.notificationDeliveryAdvanced")}</summary>
            {config.kind === "resend" ? (
              <>
                <SinkTextField
                  label={t("settings.notificationDeliverySinkFrom")}
                  type="email"
                  field="from"
                  value={sink.from_address}
                  syncScope={syncScope}
                  placeholder={DEFAULT_RESEND_FROM_ADDRESS}
                  testId="notification-delivery-sink-from"
                  onChange={(from_address) => change({ ...sink, from_address })}
                />
                <SinkTextField
                  label={t("settings.notificationDeliverySinkName")}
                  field="name"
                  value={sink.sender_name}
                  syncScope={syncScope}
                  placeholder={DEFAULT_NOTIFICATION_SENDER_NAME}
                  testId="notification-delivery-sink-name"
                  onChange={(sender_name) => change({ ...sink, sender_name })}
                />
                <p className="profile-settings-hint" data-testid="notification-delivery-resend-hint">
                  {t("settings.notificationDeliveryResendHint")}
                </p>
              </>
            ) : null}
            <SinkTextField
              label={t("settings.notificationDeliverySinkEndpoint")}
              type="url"
              field="endpoint"
              value={config.endpoint}
              syncScope={syncScope}
              testId="notification-delivery-sink-endpoint"
              onChange={(endpoint) => change({ ...sink, config: { ...config, endpoint } })}
            />
            <p className="profile-settings-hint">
              {t("settings.notificationDeliverySinkEndpointDefault")}
            </p>
          </details>
        ) : null}
      </section>

      <SinkTargets
        sink={sink}
        rooms={rooms}
        spaces={spaces}
        keywordEditor={keywordEditor}
        onChange={change}
      />

      <SinkContent
        sink={sink}
        presets={presets}
        syncScope={syncScope}
        textAvailable={!serverChan || serverChanHasKey}
        needsConsent={needsConsent}
        consentChecked={consentChecked}
        onConsentChange={(checked) => {
          setConsentChecked(checked);
          setProblem(null);
        }}
        onChange={change}
      />

      <section
        className="notification-delivery-group"
        aria-label={t("settings.notificationDeliveryTiming")}
        data-testid="notification-delivery-timing"
      >
        <h5 className="settings-subheading">{t("settings.notificationDeliveryTiming")}</h5>
        <SecondsField
          label={t("settings.notificationDeliveryDwell")}
          hint={t("settings.notificationDeliveryDwellDescription")}
          value={sink.timing.dwell_secs}
          field="dwell"
          syncScope={syncScope}
          testId="notification-delivery-dwell"
          onChange={(dwell_secs) => change({ ...sink, timing: { ...sink.timing, dwell_secs } })}
        />
        <SecondsField
          label={t("settings.notificationDeliveryInterval")}
          hint={t("settings.notificationDeliveryIntervalDescription")}
          value={sink.timing.min_interval_secs}
          field="interval"
          syncScope={syncScope}
          testId="notification-delivery-interval"
          onChange={(min_interval_secs) =>
            change({ ...sink, timing: { ...sink.timing, min_interval_secs } })
          }
        />
        <div className="settings-toggle-list">
          <ToggleRow
            label={t("settings.notificationDeliveryOnlyUnfocused")}
            testId="notification-delivery-only-unfocused"
            checked={sink.timing.only_when_unfocused}
            onToggle={(only_when_unfocused) =>
              change({ ...sink, timing: { ...sink.timing, only_when_unfocused } })
            }
          />
        </div>
      </section>

      {saved ? (
        <section
          className="notification-delivery-group"
          aria-label={t("settings.notificationDeliveryOutcome")}
          data-testid="notification-delivery-outcome"
        >
          <h5 className="settings-subheading">{t("settings.notificationDeliveryOutcome")}</h5>
          <p className="settings-status-text" data-testid="notification-delivery-sink-setup" data-setup={setup}>
            {t(setupMessage(setup))}
          </p>
          <SinkOutcome sinkState={sinkState} />
          <p className="profile-settings-hint">{t("settings.notificationDeliveryAcceptedHint")}</p>
          <div className="session-actions">
            <button
              className="trust-action-button secondary"
              type="button"
              data-testid="notification-delivery-sink-test"
              disabled={testing || !sinkState?.secret_configured}
              onClick={() => void onTest()}
            >
              <Send size={14} aria-hidden="true" />
              <span>
                {testing
                  ? t("settings.notificationDeliverySinkTestSending")
                  : t("settings.notificationDeliverySinkTest")}
              </span>
            </button>
            <button
              className="trust-action-button secondary notification-delivery-remove"
              type="button"
              data-testid="notification-delivery-sink-remove"
              aria-label={`${t("settings.notificationDeliverySinkRemove")}: ${service}`}
              onClick={onRemove}
            >
              <Trash2 size={14} aria-hidden="true" />
              <span>{t("settings.notificationDeliverySinkRemove")}</span>
            </button>
          </div>
          {!sinkState?.secret_configured ? (
            <p className="profile-settings-hint" data-testid="notification-delivery-sink-test-needs-secret">
              {t("settings.notificationDeliveryTestNeedsSecret")}
            </p>
          ) : null}
        </section>
      ) : null}

      <div className="notification-delivery-footer" data-testid="notification-delivery-footer">
        <p
          className="notification-delivery-footer-status"
          role="status"
          data-testid="notification-delivery-save-status"
          data-state={problem ? "invalid" : saveState.kind}
        >
          {saveState.kind === "saving"
            ? t("settings.notificationDeliverySaving")
            : saveState.kind === "failed"
              ? t("settings.notificationDeliverySaveFailed")
              : problemMessage ?? (dirty && !isNew ? t("settings.notificationDeliveryUnsaved") : "")}
        </p>
        <div className="notification-delivery-footer-actions">
          <button
            className="trust-action-button secondary"
            type="button"
            data-testid="notification-delivery-discard"
            disabled={saving || !dirty}
            onClick={discard}
          >
            {t("settings.notificationDeliveryDiscard")}
          </button>
          <button
            className="trust-action-button"
            type="button"
            data-testid="notification-delivery-save"
            disabled={saving || !dirty}
            aria-busy={saving ? true : undefined}
            onClick={() => void save()}
          >
            {saving ? t("settings.notificationDeliverySaving") : t("settings.notificationDeliverySave")}
          </button>
        </div>
      </div>
    </article>
  );
}

function SinkOutcome({ sinkState }: { sinkState: NotificationDeliverySinkState | null }) {
  const outcome = sinkState?.last_outcome ?? null;
  const message = notificationDeliveryOutcomeMessage(outcome);
  if (!outcome || !message) {
    return (
      <p className="settings-status-text" data-testid="notification-delivery-sink-outcome">
        {t("settings.notificationDeliveryNeverSent")}
      </p>
    );
  }
  return (
    <p
      className="settings-status-text"
      data-testid="notification-delivery-sink-outcome"
      data-outcome={outcome.kind}
    >
      {t("settings.notificationDeliveryLastAttempt", { time: formatAttemptTime(outcome.at_ms) })}{" "}
      {t(message.id, message.values)}
    </p>
  );
}

/** The ServerChan Turbo setup the user does on sct.ftqq.com, and the service's limits. */
function ServerChanSetup() {
  return (
    <div data-testid="notification-delivery-serverchan-setup">
      <p className="profile-settings-hint">{t("settings.notificationDeliveryServerChanIntro")}</p>
      <ol className="settings-steps" data-testid="notification-delivery-serverchan-steps">
        <li>{t("settings.notificationDeliveryServerChanStepLogin")}</li>
        <li>{t("settings.notificationDeliveryServerChanStepSendKey")}</li>
        <li>{t("settings.notificationDeliveryServerChanStepChannel")}</li>
      </ol>
      <p className="profile-settings-hint">
        <button
          className="profile-settings-action"
          type="button"
          data-testid="notification-delivery-serverchan-link"
          onClick={() => void openExternalHttpUrl(SERVERCHAN_SETUP_URL)}
        >
          {t("settings.notificationDeliveryServerChanOpen")}
        </button>
      </p>
      <p className="profile-settings-hint" data-testid="notification-delivery-serverchan-limits">
        {t("settings.notificationDeliveryServerChanLimits")}
      </p>
    </div>
  );
}

/**
 * Targets of one destination: the three categories, the account's keyword rules, and the
 * rooms and spaces watched for it (#1460).
 */
function SinkTargets({
  sink,
  rooms,
  spaces,
  keywordEditor,
  onChange
}: {
  sink: NotificationSinkSettings;
  rooms: readonly RoomSummary[];
  spaces: readonly SpaceSummary[];
  keywordEditor: ReactNode;
  onChange: (sink: NotificationSinkSettings) => void;
}) {
  const targets = sink.targets;
  const watched = notificationTargetsForLabel({ watched: targets.watched, rooms, spaces });
  const watchedKeys = new Set(watched.map((target) => `${target.kind}:${target.id}`));
  const addableRooms = rooms.filter((room) => !watchedKeys.has(`room:${room.room_id}`));
  const addableSpaces = spaces.filter((space) => !watchedKeys.has(`space:${space.space_id}`));
  const atLimit = targets.watched.length >= MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS;

  function setWatched(next: NotificationDeliveryTarget[]) {
    onChange({ ...sink, targets: { ...targets, watched: next } });
  }

  function removeWatched(kind: "room" | "space", id: string) {
    setWatched(
      targets.watched.filter((candidate) =>
        candidate.kind === "room" ? !(kind === "room" && candidate.room_id === id) : !(kind === "space" && candidate.space_id === id)
      )
    );
  }

  return (
    <section
      className="notification-delivery-group"
      aria-label={t("settings.notificationDeliveryTriggers")}
      data-testid="notification-delivery-targets"
    >
      <h5 className="settings-subheading">{t("settings.notificationDeliveryTriggers")}</h5>
      <div className="settings-toggle-list">
        <ToggleRow
          label={t("settings.notificationDeliveryDirectMessages")}
          testId="notification-delivery-direct-messages"
          checked={targets.direct_messages}
          onToggle={(direct_messages) => onChange({ ...sink, targets: { ...targets, direct_messages } })}
        />
        <ToggleRow
          label={t("settings.notificationDeliveryMentions")}
          testId="notification-delivery-mentions"
          checked={targets.mentions}
          onToggle={(mentions) => onChange({ ...sink, targets: { ...targets, mentions } })}
        />
        <ToggleRow
          label={t("settings.notificationDeliveryKeywords")}
          testId="notification-delivery-keywords"
          checked={targets.keywords}
          onToggle={(keywords) => onChange({ ...sink, targets: { ...targets, keywords } })}
        />
      </div>
      <p className="profile-settings-hint" data-testid="notification-delivery-keywords-hint">
        {t("settings.notificationDeliveryKeywordsHint")}
      </p>
      {keywordEditor}

      <h5 className="settings-subheading">{t("settings.notificationDeliveryWatched")}</h5>
      <p className="profile-settings-hint" data-testid="notification-delivery-watched-description">
        {t("settings.notificationDeliveryWatchedDescription")}
      </p>
      {watched.length > 0 ? (
        <ul className="settings-detail-list" data-testid="notification-delivery-watched">
          {watched.map((target) => {
            const label = target.label ?? t("settings.notificationDeliveryWatchedUnknown");
            return (
              <li
                className="settings-detail-row"
                key={`${target.kind}:${target.id}`}
                data-testid="notification-delivery-watched-row"
                data-kind={target.kind}
              >
                <span dir="auto">{label}</span>
                <small>
                  {target.kind === "room"
                    ? t("settings.notificationDeliveryWatchedRooms")
                    : t("settings.notificationDeliveryWatchedSpaces")}
                </small>
                <button
                  className="profile-settings-action"
                  type="button"
                  aria-label={t("settings.notificationDeliveryWatchedRemove", { room: label })}
                  data-testid="notification-delivery-watched-remove"
                  onClick={() => removeWatched(target.kind, target.id)}
                >
                  {t("settings.notificationDeliverySinkRemove")}
                </button>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="settings-status-text" data-testid="notification-delivery-watched-none">
          {t("settings.notificationDeliveryWatchedEmpty")}
        </p>
      )}
      <label className="profile-settings-field">
        <span>{t("settings.notificationDeliveryWatchedAdd")}</span>
        <select
          data-testid="notification-delivery-watched-add"
          disabled={atLimit}
          value=""
          onChange={(event) => {
            const [kind, ...rest] = event.currentTarget.value.split(":");
            const id = rest.join(":");
            if (!id) {
              return;
            }
            setWatched([
              ...targets.watched,
              kind === "space" ? { kind: "space", space_id: id } : { kind: "room", room_id: id }
            ]);
          }}
        >
          <option value="">{t("settings.notificationDeliveryWatchedPick")}</option>
          {addableRooms.length > 0 ? (
            <optgroup label={t("settings.notificationDeliveryWatchedRooms")}>
              {addableRooms.map((room) => (
                <option key={room.room_id} value={`room:${room.room_id}`}>
                  {roomDisplayLabel(room)}
                </option>
              ))}
            </optgroup>
          ) : null}
          {addableSpaces.length > 0 ? (
            <optgroup label={t("settings.notificationDeliveryWatchedSpaces")}>
              {addableSpaces.map((space) => (
                <option key={space.space_id} value={`space:${space.space_id}`}>
                  {space.display_name}
                </option>
              ))}
            </optgroup>
          ) : null}
        </select>
      </label>
      {atLimit ? (
        <p className="profile-settings-hint" data-testid="notification-delivery-watched-limit">
          {t("settings.notificationDeliveryWatchedLimit", {
            count: MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS
          })}
        </p>
      ) : null}
    </section>
  );
}

type ContentMode = NotificationDeliveryContent["kind"];

/**
 * What one destination's messages contain (#1460, #1468). A text mode needs a consent bound
 * to this sink kind, endpoint and recipient. The plaintext warning is shown inline with a
 * checkbox; Save carries the answer as `acceptContentConsent` and Rust mints the consent.
 * This component never builds one: choosing names only drops the stored one instead.
 */
function SinkContent({
  sink,
  presets,
  syncScope,
  textAvailable,
  needsConsent,
  consentChecked,
  onConsentChange,
  onChange
}: {
  sink: NotificationSinkSettings;
  presets: readonly NotificationDeliveryPresetState[];
  syncScope: string;
  textAvailable: boolean;
  needsConsent: boolean;
  consentChecked: boolean;
  onConsentChange: (checked: boolean) => void;
  onChange: (sink: NotificationSinkSettings) => void;
}) {
  const content = sink.content;
  const service = notificationDeliveryServiceLabel(sink, presets);
  const budget = notificationTextBudget(sink);

  function choose(mode: ContentMode) {
    if (mode === content.kind) {
      return;
    }
    if (mode === "namesOnly") {
      onChange({ ...sink, content: { kind: "namesOnly" }, content_consent: null });
      return;
    }
    onChange({
      ...sink,
      content:
        mode === "prefix"
          ? {
              kind: "prefix",
              graphemes: content.kind === "prefix" ? content.graphemes : DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES
            }
          : { kind: "full" }
    });
  }

  const modes: { mode: ContentMode; label: string }[] = [
    { mode: "namesOnly", label: t("settings.notificationDeliveryContentNamesOnly") },
    { mode: "prefix", label: t("settings.notificationDeliveryContentPrefix") },
    { mode: "full", label: t("settings.notificationDeliveryContentFull", { count: budget }) }
  ];

  return (
    <section
      className="notification-delivery-group"
      aria-label={t("settings.notificationDeliveryContent")}
      data-testid="notification-delivery-content"
      data-mode={content.kind}
    >
      <h5 className="settings-subheading">{t("settings.notificationDeliveryContent")}</h5>
      <div role="radiogroup" aria-label={t("settings.notificationDeliveryContent")} className="settings-control-stack">
        {modes.map(({ mode, label }) => (
          <label className="settings-detail-row" key={mode}>
            <input
              type="radio"
              name={`notification-delivery-content-${sink.id}`}
              value={mode}
              data-testid="notification-delivery-content-mode"
              data-mode={mode}
              checked={content.kind === mode}
              disabled={mode !== "namesOnly" && mode !== content.kind && !textAvailable}
              onChange={() => choose(mode)}
            />
            <span>{label}</span>
          </label>
        ))}
      </div>
      {content.kind === "prefix" ? (
        <NumberField
          label={t("settings.notificationDeliveryContentPrefixCount")}
          value={content.graphemes}
          field="prefix"
          syncScope={syncScope}
          testId="notification-delivery-content-prefix-count"
          maxLength={3}
          clamp={clampNotificationDeliveryPrefixGraphemes}
          onChange={(graphemes) => onChange({ ...sink, content: { kind: "prefix", graphemes } })}
        />
      ) : null}
      {!textAvailable ? (
        <p className="profile-settings-hint" data-testid="notification-delivery-content-needs-key">
          {t("settings.notificationDeliveryContentNeedsKey")}
        </p>
      ) : null}
      <p className="profile-settings-hint" data-testid="notification-delivery-content-limit">
        {t("settings.notificationDeliveryContentLimit", { service, count: budget })}
      </p>
      <p className="profile-settings-hint" data-testid="notification-delivery-content-fallback">
        {t("settings.notificationDeliveryContentFallback")}
      </p>
      {content.kind !== "namesOnly" ? (
        <p className="profile-settings-hint" data-testid="notification-delivery-content-plaintext">
          {t("settings.notificationDeliveryContentPlaintext", { service })}
        </p>
      ) : null}
      {needsConsent ? (
        <div
          className="notification-delivery-consent"
          role="group"
          aria-label={t("settings.notificationDeliveryConsentTitle")}
          data-testid="notification-delivery-consent"
        >
          <p className="notification-delivery-consent-title">{t("settings.notificationDeliveryConsentTitle")}</p>
          <p className="profile-settings-hint" data-testid="notification-delivery-consent-body">
            {t("settings.notificationDeliveryConsentBody", {
              service,
              destination:
                sink.config.kind === "serverChan"
                  ? t("settings.notificationDeliveryConsentServerChanRecipient")
                  : sink.destination.trim()
            })}
          </p>
          <p className="profile-settings-hint">{t("settings.notificationDeliveryConsentRebind")}</p>
          <label className="settings-detail-row">
            <input
              type="checkbox"
              data-testid="notification-delivery-consent-acknowledge"
              checked={consentChecked}
              onChange={(event) => onConsentChange(event.currentTarget.checked)}
            />
            <span>{t("settings.notificationDeliveryConsentAcknowledge")}</span>
          </label>
        </div>
      ) : null}
      <p className="profile-settings-hint" data-testid="notification-delivery-content-independent">
        {t("settings.notificationDeliveryContentIndependent")}
      </p>
    </section>
  );
}

/**
 * The account's keyword rules, next to the switch that delivers for them
 * (#1427).
 *
 * Every row is the last server projection: the component never appends or
 * removes locally, so a write Rust refuses leaves the list exactly as it was
 * and only the failure line changes. The field is the shared IME-safe text
 * entry, and the text it holds is the raw entry; Rust trims and validates.
 */
function NotificationKeywordEditor({
  rules,
  operation,
  actions
}: {
  rules: readonly NotificationKeywordRule[];
  operation: AccountNotificationsOperationState;
  actions: NotificationKeywordActions;
}) {
  const [draft, setDraft] = useState("");
  // Bumped when Rust confirms an add, so the shared control re-syncs from the
  // acknowledged (empty) value rather than keeping the typed text.
  const [draftEpoch, setDraftEpoch] = useState(0);
  // Mirrors the DOM value so the confirmation effect is idempotent: once the
  // field is already empty it changes nothing, however often `operation`
  // re-identifies.
  const draftRef = useRef("");
  const busy = operation.kind === "working" && isKeywordOperation(operation.operation);
  const failure =
    operation.kind === "failed" && isKeywordOperation(operation.operation)
      ? operation.failureKind
      : null;

  useEffect(() => {
    if (
      operation.kind === "succeeded" &&
      operation.operation.kind === "addNotificationKeyword" &&
      draftRef.current !== ""
    ) {
      draftRef.current = "";
      setDraft("");
      setDraftEpoch((epoch) => epoch + 1);
    }
  }, [operation]);

  function submitKeyword(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const pattern = draft.trim();
    if (!pattern || busy) {
      return;
    }
    actions.add(pattern);
  }

  return (
    <section
      className="settings-keyword-editor"
      aria-label={t("settings.notificationKeywordHeading")}
      data-testid="notification-keywords"
    >
      <h5 className="settings-subheading">{t("settings.notificationKeywordHeading")}</h5>
      {rules.length > 0 ? (
        <ul className="settings-detail-list" data-testid="notification-keyword-list">
          {rules.map((rule) => (
            <li
              className="settings-detail-row"
              key={rule.rule_id}
              data-testid="notification-keyword-row"
            >
              <span dir="auto">{rule.pattern}</span>
              {/* A rule another client switched off is still on the account and
                  still removable; saying so keeps it from reading as active. */}
              {!rule.enabled ? (
                <small data-testid="notification-keyword-disabled">
                  {t("settings.notificationKeywordDisabled")}
                </small>
              ) : null}
              <button
                className="profile-settings-action"
                type="button"
                aria-label={t("settings.notificationKeywordRemove", { keyword: rule.pattern })}
                data-testid="notification-keyword-remove"
                disabled={busy}
                onClick={() => actions.remove(rule.rule_id)}
              >
                {t("settings.notificationDeliverySinkRemove")}
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="settings-status-text" data-testid="notification-keyword-empty">
          {t("settings.notificationKeywordEmpty")}
        </p>
      )}
      <ImeSafeForm className="profile-settings-form" onSubmit={submitKeyword}>
        <label className="profile-settings-field">
          <span>{t("settings.notificationKeywordAdd")}</span>
          <ImeTextField
            value={draft}
            syncKey={`notification-keyword-draft:${draftEpoch}`}
            disabled={busy}
            onChange={(event) => {
              draftRef.current = event.currentTarget.value;
              setDraft(event.currentTarget.value);
            }}
            data-testid="notification-keyword-input"
          />
        </label>
        <div className="session-actions">
          <button
            className="trust-action-button primary"
            type="submit"
            data-testid="notification-keyword-add"
            disabled={busy || draft.trim().length === 0}
          >
            {t("action.add")}
          </button>
        </div>
      </ImeSafeForm>
      {failure ? (
        <p className="settings-status-text" data-testid="notification-keyword-error">
          {t(accountNotificationsFailureMessage(failure))}
        </p>
      ) : null}
      <p className="profile-settings-hint" data-testid="notification-keyword-hint">
        {t("settings.notificationKeywordHint")}
      </p>
    </section>
  );
}

function selectedPresetIsFullAddress(
  presets: readonly NotificationDeliveryPresetState[],
  presetId: string | null
): boolean {
  return Boolean(presets.find((preset) => preset.id === presetId)?.username_is_full_address);
}

function SecretHint({
  presets,
  presetId
}: {
  presets: readonly NotificationDeliveryPresetState[];
  presetId: string | null;
}) {
  const preset = presets.find((candidate) => candidate.id === presetId);
  if (!preset) {
    return null;
  }
  return (
    <p className="profile-settings-hint" data-testid="notification-delivery-add-secret-hint">
      {t(preset.secret_hint)}
    </p>
  );
}

/** A `u16` on the Rust side: an out-of-range port would fail the whole settings patch. */
function smtpPort(value: number): number {
  if (!Number.isFinite(value)) {
    return 1;
  }
  return Math.min(Math.max(Math.trunc(value), 1), 65_535);
}

function ToggleRow({
  label,
  testId,
  checked,
  onToggle
}: {
  label: string;
  testId: string;
  checked: boolean;
  onToggle: (next: boolean) => void;
}) {
  return (
    <button
      className="settings-toggle-row"
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      data-testid={testId}
      onClick={() => onToggle(!checked)}
    >
      <span className="settings-toggle-copy">
        <span className="settings-toggle-label">
          <Bell size={15} aria-hidden="true" />
          <span>{label}</span>
        </span>
      </span>
      <span className="settings-switch-track" aria-hidden="true">
        <span className="settings-switch-thumb" />
      </span>
    </button>
  );
}

