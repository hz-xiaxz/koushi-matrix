import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { ArrowLeft, Bell, ChevronRight, KeyRound, Plus, Send, Trash2 } from "lucide-react";

import { openExternalHttpUrl } from "../../backend/linkMediaRuntime";
import { getActiveLocale, t, type MessageId } from "../../i18n/messages";
import { ImeSafeForm, ImeTextField, SecureImeTextField } from "../ImeTextControl";
import { ModalDialog } from "../ModalDialog";
import {
  accountNotificationsFailureMessage,
  isKeywordOperation
} from "./AccountNotificationsSections";
import { cachedDateTimeFormat } from "../../domain/intlFormatCache";
import { roomDisplayLabel } from "../../domain/roomDisplayLabel";
import {
  DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES,
  MAX_NOTIFICATION_DELIVERY_SINKS,
  MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS,
  SERVERCHAN_SETUP_URL,
  clampNotificationDeliveryPrefixGraphemes,
  clampNotificationDeliverySeconds,
  createNotificationSinkId,
  newNotificationSink,
  notificationConsentForCurrent,
  notificationConsentRecipient,
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
 * Typed commands of the device-local notification delivery section (#1427, #1460).
 *
 * `update` carries the whole delivery block, because that is the shape Rust's
 * `SettingsPatch.delivery` replaces; everything else is one of the three sink commands the
 * app-scoped state owns. None of them returns the value that was written.
 */
export interface NotificationDeliveryActions {
  update: (delivery: NotificationDeliverySettings) => void;
  setSecret: (sinkId: string, secret: string) => Promise<void>;
  clearSecret: (sinkId: string) => Promise<void>;
  test: (sinkId: string) => Promise<void>;
}

export const noopNotificationDeliveryActions: NotificationDeliveryActions = {
  update: () => undefined,
  setSecret: async () => undefined,
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
 * Which screen of the section is open. A destination being added is a local draft until
 * "Save destination" commits it; after that the detail view follows Rust's copy, and the
 * committed draft stands in only until the snapshot that carries it arrives.
 */
type DeliveryView =
  | { kind: "list" }
  | { kind: "pick" }
  | { kind: "draft"; sink: NotificationSinkSettings }
  | { kind: "detail"; sinkId: string; pending: NotificationSinkSettings | null };

/**
 * Settings -> Notifications, "Sent from this device" (#1460).
 *
 * A list of destinations, each opening a detail view that owns its own targets, content,
 * timing and outcome. Everything shown is Rust state: `delivery` is the settings block,
 * `state` is the app-scoped runtime half (whether a credential exists, and the last outcome
 * per sink), and `rooms`/`spaces` supply the labels the watch list renders. This component
 * computes no dwell, starts no timer, assembles no message, and never reads a secret back.
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
  const [busySinkId, setBusySinkId] = useState<string | null>(null);

  const sinkStates = useMemo(() => {
    const map = new Map<string, NotificationDeliverySinkState>();
    for (const sinkState of state.sinks) {
      map.set(sinkState.sink_id, sinkState);
    }
    return map;
  }, [state.sinks]);

  function replaceSink(next: NotificationSinkSettings) {
    actions.update({
      ...delivery,
      sinks: delivery.sinks.map((sink) => (sink.id === next.id ? next : sink))
    });
  }

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

  function commitDraft(draft: NotificationSinkSettings) {
    const sink = trimSinkFields(draft);
    if (!notificationSinkFieldsReady(sink) || delivery.sinks.length >= MAX_NOTIFICATION_DELIVERY_SINKS) {
      return;
    }
    actions.update({ ...delivery, sinks: [...delivery.sinks, sink] });
    setView({ kind: "detail", sinkId: sink.id, pending: sink });
  }

  async function runBusy(sinkId: string, run: () => Promise<void>) {
    setBusySinkId(sinkId);
    try {
      await run();
    } finally {
      setBusySinkId(null);
    }
  }

  let body;
  if (view.kind === "pick") {
    body = (
      <div className="profile-settings-form" data-testid="notification-delivery-pick">
        <h4 className="settings-subheading">{t("settings.notificationDeliveryAddSink")}</h4>
        <div className="settings-detail-list">
          {SINK_KINDS.map((kind) => (
            <button
              key={kind}
              className="settings-detail-row notification-delivery-row"
              type="button"
              data-testid="notification-delivery-pick-kind"
              data-kind={kind}
              onClick={() =>
                setView({ kind: "draft", sink: newNotificationSink(createNotificationSinkId(), kind) })
              }
            >
              <Send size={15} aria-hidden="true" />
              <span>{sinkKindLabel(kind)}</span>
              <ChevronRight size={15} aria-hidden="true" />
            </button>
          ))}
        </div>
        <div className="session-actions">
          <button
            className="trust-action-button secondary"
            type="button"
            data-testid="notification-delivery-pick-cancel"
            onClick={() => setView({ kind: "list" })}
          >
            {t("action.cancel")}
          </button>
        </div>
      </div>
    );
  } else if (view.kind === "draft" || view.kind === "detail") {
    const draft = view.kind === "draft";
    const sink = draft
      ? view.sink
      : delivery.sinks.find((candidate) => candidate.id === view.sinkId) ?? view.pending;
    if (!sink) {
      // Rust dropped the destination (another window removed it, or the patch was refused).
      body = renderSinkList();
    } else {
      body = (
        <NotificationSinkDetail
          key={sink.id}
          sink={sink}
          draft={draft}
          sinkState={draft ? null : sinkStates.get(sink.id) ?? null}
          presets={state.presets}
          rooms={rooms}
          spaces={spaces}
          busy={busySinkId === sink.id}
          keywordEditor={
            keywordRules !== undefined && keywordOperation !== undefined ? (
              <NotificationKeywordEditor
                rules={keywordRules}
                operation={keywordOperation}
                actions={keywordActions}
              />
            ) : null
          }
          onChange={(next) => (draft ? setView({ kind: "draft", sink: next }) : replaceSink(next))}
          onSaveDraft={() => commitDraft(sink)}
          onBack={() => setView({ kind: "list" })}
          onRemove={() => void removeSink(sink.id)}
          onSetSecret={(secret) => runBusy(sink.id, () => actions.setSecret(sink.id, secret))}
          onClearSecret={() => {
            void actions.clearSecret(sink.id);
          }}
          onTest={() => runBusy(sink.id, () => actions.test(sink.id))}
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
          <div className="settings-detail-list" data-testid="notification-delivery-sinks">
            {delivery.sinks.map((sink) => {
              const sinkState = sinkStates.get(sink.id) ?? null;
              const setup = notificationSinkSetup(sink, sinkState);
              const failed = notificationDeliverySinkFailed(sinkState?.last_outcome ?? null);
              const service = notificationDeliveryServiceLabel(sink, state.presets);
              const status =
                setup === "configured"
                  ? t("settings.notificationDeliveryConfigured")
                  : t("settings.notificationDeliveryNotConfigured");
              return (
                <button
                  key={sink.id}
                  className="settings-detail-row notification-delivery-row"
                  type="button"
                  data-testid="notification-delivery-sink"
                  data-sink-id={sink.id}
                  data-setup={setup}
                  data-state={failed ? "failed" : "ok"}
                  aria-label={`${service}: ${status}`}
                  onClick={() => setView({ kind: "detail", sinkId: sink.id, pending: null })}
                >
                  <Send size={15} aria-hidden="true" />
                  <span className="settings-toggle-copy">
                    <span dir="auto">{service}</span>
                    {sink.config.kind !== "serverChan" && sink.destination ? (
                      <small dir="auto">{sink.destination}</small>
                    ) : null}
                  </span>
                  <small data-testid="notification-delivery-sink-status">{status}</small>
                  {failed ? (
                    <small data-testid="notification-delivery-sink-failed">
                      {t("settings.notificationDeliverySinkFailed")}
                    </small>
                  ) : null}
                  <ChevronRight size={15} aria-hidden="true" />
                </button>
              );
            })}
          </div>
        )}
        <div className="session-actions">
          <button
            className="trust-action-button secondary"
            type="button"
            data-testid="notification-delivery-add"
            disabled={delivery.sinks.length >= MAX_NOTIFICATION_DELIVERY_SINKS}
            onClick={() => setView({ kind: "pick" })}
          >
            <Plus size={14} aria-hidden="true" />
            <span>{t("settings.notificationDeliveryAddSink")}</span>
          </button>
        </div>
        {delivery.sinks.length >= MAX_NOTIFICATION_DELIVERY_SINKS ? (
          <p className="profile-settings-hint" data-testid="notification-delivery-sinks-limit">
            {t("settings.notificationDeliverySinkLimit", { count: MAX_NOTIFICATION_DELIVERY_SINKS })}
          </p>
        ) : null}
      </>
    );
  }

  return (
    <section
      className="settings-section"
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

/**
 * One text field of a destination. A draft commits on every keystroke, so "Save
 * destination" is enabled as soon as the form is complete; a saved destination commits on
 * blur, and its sync key names the committed value so a value Rust rewrote replaces the
 * typed one.
 */
function SinkTextField({
  label,
  value,
  field,
  sinkId,
  draft,
  testId,
  type = "text",
  placeholder,
  onCommit
}: {
  label: string;
  value: string;
  field: string;
  sinkId: string;
  draft: boolean;
  testId: string;
  type?: "text" | "email" | "url";
  placeholder?: string;
  onCommit: (value: string) => void;
}) {
  return (
    <label className="profile-settings-field">
      <span>{label}</span>
      <ImeTextField
        type={type}
        value={value}
        autoComplete="off"
        placeholder={placeholder}
        syncKey={draft ? `sink-${field}:${sinkId}` : `sink-${field}:${sinkId}:${value}`}
        data-testid={testId}
        onChange={draft ? (event) => onCommit(event.currentTarget.value) : undefined}
        onBlur={
          draft
            ? undefined
            : (event) => {
                const next = event.currentTarget.value.trim();
                if (next !== value) {
                  onCommit(next);
                }
              }
        }
      />
    </label>
  );
}

function SecondsField({
  label,
  hint,
  value,
  field,
  sinkId,
  testId,
  onCommit
}: {
  label: string;
  hint: string;
  value: number;
  field: string;
  sinkId: string;
  testId: string;
  onCommit: (value: number) => void;
}) {
  return (
    <>
      <label className="profile-settings-field">
        <span>{label}</span>
        <ImeTextField
          type="text"
          inputMode="numeric"
          maxLength={6}
          value={String(value)}
          aria-label={label}
          // The key names the committed value: Rust is what bounds the field, so a
          // rejected entry has to be replaced by the bound the settings now hold.
          syncKey={`sink-${field}:${sinkId}:${value}`}
          data-testid={testId}
          onBlur={(event) => {
            const next = clampNotificationDeliverySeconds(Number(event.currentTarget.value));
            if (next !== value) {
              onCommit(next);
            }
          }}
        />
      </label>
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
  return cachedDateTimeFormat(getActiveLocale(), { dateStyle: "medium", timeStyle: "short" }).format(
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

/**
 * The detail view of one destination: credentials, targets, content, timing and outcome,
 * in that order (#1460). A draft shows only what can be decided before the destination
 * exists in Rust's settings; the credential, content and outcome need a saved sink,
 * because the credential store is keyed by it and consent binds to its revision.
 */
function NotificationSinkDetail({
  sink,
  draft,
  sinkState,
  presets,
  rooms,
  spaces,
  busy,
  keywordEditor,
  onChange,
  onSaveDraft,
  onBack,
  onRemove,
  onSetSecret,
  onClearSecret,
  onTest
}: {
  sink: NotificationSinkSettings;
  draft: boolean;
  sinkState: NotificationDeliverySinkState | null;
  presets: readonly NotificationDeliveryPresetState[];
  rooms: readonly RoomSummary[];
  spaces: readonly SpaceSummary[];
  busy: boolean;
  keywordEditor: ReactNode;
  onChange: (sink: NotificationSinkSettings) => void;
  onSaveDraft: () => void;
  onBack: () => void;
  onRemove: () => void;
  onSetSecret: (secret: string) => Promise<void>;
  onClearSecret: () => void;
  onTest: () => Promise<void>;
}) {
  // Held as a local const so the discriminated union stays narrowed inside the handlers.
  const config = sink.config;
  const service = notificationDeliveryServiceLabel(sink, presets);
  const setup = notificationSinkSetup(sink, sinkState);

  return (
    <article
      className="settings-section"
      data-testid="notification-delivery-detail"
      data-sink-id={sink.id}
      data-kind={config.kind}
      data-draft={draft ? "true" : "false"}
    >
      <div className="session-actions">
        <button
          className="profile-settings-action"
          type="button"
          data-testid="notification-delivery-back"
          onClick={onBack}
        >
          <ArrowLeft size={14} aria-hidden="true" />
          <span>{t("settings.notificationDeliveryBack")}</span>
        </button>
      </div>
      <h4 className="settings-subheading" dir="auto">
        {service}
      </h4>
      {draft ? (
        <p className="profile-settings-hint" data-testid="notification-delivery-draft-hint">
          {t("settings.notificationDeliveryDraftHint")}
        </p>
      ) : null}

      <section
        className="settings-section"
        aria-label={t("settings.notificationDeliveryCredentials")}
        data-testid="notification-delivery-credentials"
      >
        <h4 className="settings-subheading">{t("settings.notificationDeliveryCredentials")}</h4>
        {config.kind === "serverChan" ? (
          <ServerChanSetup />
        ) : (
          <>
            {config.kind === "resend" ? (
              <p className="profile-settings-hint" data-testid="notification-delivery-resend-hint">
                {t("settings.notificationDeliveryResendHint")}
              </p>
            ) : null}
            <SinkTextField
              label={t("settings.notificationDeliverySinkFrom")}
              type="email"
              field="from"
              value={sink.from_address}
              sinkId={sink.id}
              draft={draft}
              testId="notification-delivery-sink-from"
              onCommit={(from_address) => onChange({ ...sink, from_address })}
            />
            <SinkTextField
              label={t("settings.notificationDeliverySinkName")}
              field="name"
              value={sink.sender_name}
              sinkId={sink.id}
              draft={draft}
              testId="notification-delivery-sink-name"
              onCommit={(sender_name) => onChange({ ...sink, sender_name })}
            />
            <SinkTextField
              label={t("settings.notificationDeliverySinkTo")}
              type="email"
              field="to"
              value={sink.destination}
              sinkId={sink.id}
              draft={draft}
              testId="notification-delivery-sink-to"
              onCommit={(destination) => onChange({ ...sink, destination })}
            />
          </>
        )}
        {config.kind === "smtp" ? (
          <>
            <label className="profile-settings-field">
              <span>{t("settings.notificationDeliverySinkPreset")}</span>
              <select
                data-testid="notification-delivery-sink-preset"
                value={config.preset ?? ""}
                onChange={(event) => {
                  const preset = presets.find((candidate) => candidate.id === event.currentTarget.value);
                  onChange({
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
              label={t("settings.notificationDeliverySinkHost")}
              field="host"
              value={config.host}
              sinkId={sink.id}
              draft={draft}
              testId="notification-delivery-sink-host"
              onCommit={(host) => onChange({ ...sink, config: { ...config, host } })}
            />
            <label className="profile-settings-field">
              <span>{t("settings.notificationDeliverySinkPort")}</span>
              <ImeTextField
                type="text"
                inputMode="numeric"
                maxLength={5}
                value={String(config.port)}
                syncKey={`sink-port:${sink.id}:${config.port}`}
                data-testid="notification-delivery-sink-port"
                onBlur={(event) => {
                  const port = smtpPort(Number(event.currentTarget.value));
                  if (port !== config.port) {
                    onChange({ ...sink, config: { ...config, port } });
                  }
                }}
              />
            </label>
            <label className="profile-settings-field">
              <span>{t("settings.notificationDeliverySinkTls")}</span>
              <select
                data-testid="notification-delivery-sink-tls"
                value={config.tls}
                onChange={(event) =>
                  onChange({ ...sink, config: { ...config, tls: event.currentTarget.value as SmtpTls } })
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
              sinkId={sink.id}
              draft={draft}
              testId="notification-delivery-sink-username"
              placeholder={
                selectedPresetIsFullAddress(presets, config.preset ?? null)
                  ? t("settings.notificationDeliverySinkUsernameFullAddress")
                  : undefined
              }
              onCommit={(username) => onChange({ ...sink, config: { ...config, username } })}
            />
            <SecretHint presets={presets} presetId={config.preset ?? null} />
          </>
        ) : null}

        <SinkSecretField
          sink={sink}
          draft={draft}
          sinkState={sinkState}
          busy={busy}
          onSetSecret={onSetSecret}
          onClearSecret={onClearSecret}
        />

        {config.kind !== "smtp" ? (
          <details className="settings-advanced" data-testid="notification-delivery-advanced">
            <summary>{t("settings.notificationDeliveryAdvanced")}</summary>
            <SinkTextField
              label={t("settings.notificationDeliverySinkEndpoint")}
              type="url"
              field="endpoint"
              value={config.endpoint}
              sinkId={sink.id}
              draft={draft}
              testId="notification-delivery-sink-endpoint"
              onCommit={(endpoint) => onChange({ ...sink, config: { ...config, endpoint } })}
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
        onChange={onChange}
      />

      {draft ? null : <SinkContent sink={sink} presets={presets} onChange={onChange} />}

      <section
        className="settings-section"
        aria-label={t("settings.notificationDeliveryTiming")}
        data-testid="notification-delivery-timing"
      >
        <h4 className="settings-subheading">{t("settings.notificationDeliveryTiming")}</h4>
        <SecondsField
          label={t("settings.notificationDeliveryDwell")}
          hint={t("settings.notificationDeliveryDwellDescription")}
          value={sink.timing.dwell_secs}
          field="dwell"
          sinkId={sink.id}
          testId="notification-delivery-dwell"
          onCommit={(dwell_secs) => onChange({ ...sink, timing: { ...sink.timing, dwell_secs } })}
        />
        <SecondsField
          label={t("settings.notificationDeliveryInterval")}
          hint={t("settings.notificationDeliveryIntervalDescription")}
          value={sink.timing.min_interval_secs}
          field="interval"
          sinkId={sink.id}
          testId="notification-delivery-interval"
          onCommit={(min_interval_secs) =>
            onChange({ ...sink, timing: { ...sink.timing, min_interval_secs } })
          }
        />
        <div className="settings-toggle-list">
          <ToggleRow
            label={t("settings.notificationDeliveryOnlyUnfocused")}
            testId="notification-delivery-only-unfocused"
            checked={sink.timing.only_when_unfocused}
            onToggle={(only_when_unfocused) =>
              onChange({ ...sink, timing: { ...sink.timing, only_when_unfocused } })
            }
          />
        </div>
      </section>

      {draft ? (
        <div className="session-actions">
          <button
            className="trust-action-button secondary"
            type="button"
            data-testid="notification-delivery-draft-cancel"
            onClick={onBack}
          >
            {t("action.cancel")}
          </button>
          <button
            className="trust-action-button primary"
            type="button"
            data-testid="notification-delivery-draft-save"
            disabled={!notificationSinkFieldsReady(trimSinkFields(sink))}
            onClick={onSaveDraft}
          >
            {t("settings.notificationDeliverySaveDestination")}
          </button>
        </div>
      ) : (
        <>
          <section
            className="settings-section"
            aria-label={t("settings.notificationDeliveryOutcome")}
            data-testid="notification-delivery-outcome"
          >
            <h4 className="settings-subheading">{t("settings.notificationDeliveryOutcome")}</h4>
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
                disabled={busy || !sinkState?.secret_configured}
                onClick={() => void onTest()}
              >
                <Send size={14} aria-hidden="true" />
                <span>
                  {busy
                    ? t("settings.notificationDeliverySinkTestSending")
                    : t("settings.notificationDeliverySinkTest")}
                </span>
              </button>
            </div>
            {!sinkState?.secret_configured ? (
              <p className="profile-settings-hint" data-testid="notification-delivery-sink-test-needs-secret">
                {t("settings.notificationDeliveryTestNeedsSecret")}
              </p>
            ) : null}
          </section>
          <div className="session-actions">
            <button
              className="trust-action-button secondary"
              type="button"
              data-testid="notification-delivery-sink-remove"
              aria-label={`${t("settings.notificationDeliverySinkRemove")}: ${service}`}
              onClick={onRemove}
            >
              <Trash2 size={14} aria-hidden="true" />
              <span>{t("settings.notificationDeliverySinkRemove")}</span>
            </button>
          </div>
        </>
      )}
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

/**
 * The write-only credential field. The value is read from the DOM at the explicit save
 * boundary and cleared there; it never enters React state and is never written back from
 * the snapshot. For ServerChan only the *kind* of shape problem is kept in state, so the
 * field can say why before the command leaves; Rust refuses the same values again.
 */
function SinkSecretField({
  sink,
  draft,
  sinkState,
  busy,
  onSetSecret,
  onClearSecret
}: {
  sink: NotificationSinkSettings;
  draft: boolean;
  sinkState: NotificationDeliverySinkState | null;
  busy: boolean;
  onSetSecret: (secret: string) => Promise<void>;
  onClearSecret: () => void;
}) {
  const secretRef = useRef<HTMLInputElement | null>(null);
  const [problem, setProblem] = useState<ServerChanSendKeyProblem | null>(null);
  const serverChan = sink.config.kind === "serverChan";
  const label = secretLabel(sink.config.kind);
  const secretConfigured = Boolean(sinkState?.secret_configured);
  // Rust says when it deleted a stored credential because the sink stopped being the server
  // or account it was entered for, so the empty field is explained rather than read as data
  // the user lost.
  const secretGone = Boolean(sinkState?.secret_retired) && !secretConfigured;

  if (draft) {
    return (
      <p className="profile-settings-hint" data-testid="notification-delivery-secret-after-save">
        {t("settings.notificationDeliverySecretAfterSave", { label })}
      </p>
    );
  }

  return (
    <>
      <label className="profile-settings-field">
        <span>{label}</span>
        <SecureImeTextField
          ref={secretRef}
          autoComplete="off"
          syncKey={`sink-secret:${sink.id}`}
          data-testid="notification-delivery-sink-secret"
          aria-label={label}
          aria-invalid={problem ? true : undefined}
          onChange={
            serverChan
              ? (event) => {
                  const value = event.currentTarget.value;
                  setProblem(value ? serverChanSendKeyProblem(value) : null);
                }
              : undefined
          }
        />
      </label>
      {problem ? (
        <p className="settings-status-text" role="alert" data-testid="notification-delivery-sink-secret-problem">
          {t(sendKeyProblemMessage(problem))}
        </p>
      ) : null}
      <p className="profile-settings-hint" data-testid="notification-delivery-sink-secret-state">
        {secretConfigured
          ? t("settings.notificationDeliverySinkSecretSet")
          : t("settings.notificationDeliverySinkSecretMissing")}
      </p>
      {secretGone ? (
        <p className="profile-settings-hint" data-testid="notification-delivery-sink-secret-retired">
          {t("settings.notificationDeliverySinkSecretRetired")}
        </p>
      ) : null}
      <div className="session-actions">
        <button
          className="trust-action-button secondary"
          type="button"
          data-testid="notification-delivery-sink-save-secret"
          disabled={busy || problem !== null}
          onClick={() => {
            const field = secretRef.current;
            if (!field) {
              return;
            }
            const secret = serverChan ? field.value.trim() : field.value;
            if (serverChan && secret && serverChanSendKeyProblem(secret) !== null) {
              setProblem(serverChanSendKeyProblem(secret));
              return;
            }
            field.value = "";
            if (!secret) {
              return;
            }
            void onSetSecret(secret);
          }}
        >
          <KeyRound size={14} aria-hidden="true" />
          <span>{t("settings.notificationDeliverySinkSaveSecret")}</span>
        </button>
        {secretConfigured ? (
          <button
            className="trust-action-button secondary"
            type="button"
            data-testid="notification-delivery-sink-clear-secret"
            disabled={busy}
            onClick={onClearSecret}
          >
            {t("settings.notificationDeliverySinkClearSecret")}
          </button>
        ) : null}
      </div>
    </>
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
      className="settings-section"
      aria-label={t("settings.notificationDeliveryTriggers")}
      data-testid="notification-delivery-targets"
    >
      <h4 className="settings-subheading">{t("settings.notificationDeliveryTriggers")}</h4>
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

      <h4 className="settings-subheading">{t("settings.notificationDeliveryWatched")}</h4>
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
 * What one destination's messages contain (#1460). A text mode needs a consent bound to
 * this sink kind, endpoint and recipient; choosing one opens the consent dialog, and Rust
 * drops the consent (and falls back to names only) as soon as any of the three changes.
 */
function SinkContent({
  sink,
  presets,
  onChange
}: {
  sink: NotificationSinkSettings;
  presets: readonly NotificationDeliveryPresetState[];
  onChange: (sink: NotificationSinkSettings) => void;
}) {
  const [pending, setPending] = useState<NotificationDeliveryContent | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const effective = notificationEffectiveContent(sink);
  const service = notificationDeliveryServiceLabel(sink, presets);
  const budget = notificationTextBudget(sink);
  // ServerChan's recipient is the revision of the saved SendKey; before one exists there is
  // nothing a consent could name.
  const textAvailable = notificationConsentRecipient(sink) !== "";

  function choose(mode: ContentMode) {
    if (mode === effective.kind) {
      return;
    }
    if (mode === "namesOnly") {
      onChange({ ...sink, content: { kind: "namesOnly" }, content_consent: null });
      return;
    }
    const content: NotificationDeliveryContent =
      mode === "prefix"
        ? {
            kind: "prefix",
            graphemes:
              effective.kind === "prefix" ? effective.graphemes : DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES
          }
        : { kind: "full" };
    if (effective.kind !== "namesOnly") {
      // Switching between the two text modes keeps the consent this destination already has.
      onChange({ ...sink, content });
      return;
    }
    setAcknowledged(false);
    setPending(content);
  }

  function accept() {
    if (!pending) {
      return;
    }
    // The consent names the exact kind, endpoint and recipient it was given for, so a
    // later change to any of them is a new question rather than an inherited answer.
    onChange({ ...sink, content: pending, content_consent: notificationConsentForCurrent(sink, Date.now()) });
    setPending(null);
    setAcknowledged(false);
  }

  const modes: { mode: ContentMode; label: string }[] = [
    { mode: "namesOnly", label: t("settings.notificationDeliveryContentNamesOnly") },
    { mode: "prefix", label: t("settings.notificationDeliveryContentPrefix") },
    { mode: "full", label: t("settings.notificationDeliveryContentFull", { count: budget }) }
  ];

  return (
    <section
      className="settings-section"
      aria-label={t("settings.notificationDeliveryContent")}
      data-testid="notification-delivery-content"
      data-mode={effective.kind}
    >
      <h4 className="settings-subheading">{t("settings.notificationDeliveryContent")}</h4>
      <div role="radiogroup" aria-label={t("settings.notificationDeliveryContent")} className="settings-control-stack">
        {modes.map(({ mode, label }) => (
          <label className="settings-detail-row" key={mode}>
            <input
              type="radio"
              name={`notification-delivery-content-${sink.id}`}
              value={mode}
              data-testid="notification-delivery-content-mode"
              data-mode={mode}
              checked={effective.kind === mode}
              disabled={mode !== "namesOnly" && !textAvailable}
              onChange={() => choose(mode)}
            />
            <span>{label}</span>
          </label>
        ))}
      </div>
      {effective.kind === "prefix" ? (
        <label className="profile-settings-field">
          <span>{t("settings.notificationDeliveryContentPrefixCount")}</span>
          <ImeTextField
            type="text"
            inputMode="numeric"
            maxLength={3}
            value={String(effective.graphemes)}
            syncKey={`sink-prefix:${sink.id}:${effective.graphemes}`}
            data-testid="notification-delivery-content-prefix-count"
            onBlur={(event) => {
              const graphemes = clampNotificationDeliveryPrefixGraphemes(Number(event.currentTarget.value));
              if (graphemes !== effective.graphemes) {
                onChange({ ...sink, content: { kind: "prefix", graphemes } });
              }
            }}
          />
        </label>
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
      {effective.kind !== "namesOnly" ? (
        <p className="profile-settings-hint" data-testid="notification-delivery-content-plaintext">
          {t("settings.notificationDeliveryContentPlaintext", { service })}
        </p>
      ) : null}
      <p className="profile-settings-hint" data-testid="notification-delivery-content-independent">
        {t("settings.notificationDeliveryContentIndependent")}
      </p>

      {pending ? (
        <ModalDialog
          title={t("settings.notificationDeliveryConsentTitle")}
          className="dialog-box"
          onClose={() => {
            setPending(null);
            setAcknowledged(false);
          }}
        >
          <p className="profile-settings-hint" data-testid="notification-delivery-consent-body">
            {t("settings.notificationDeliveryConsentBody", {
              service,
              destination:
                sink.config.kind === "serverChan"
                  ? t("settings.notificationDeliveryConsentServerChanRecipient")
                  : sink.destination
            })}
          </p>
          <p className="profile-settings-hint">{t("settings.notificationDeliveryConsentRebind")}</p>
          <label className="settings-detail-row">
            <input
              type="checkbox"
              data-testid="notification-delivery-consent-acknowledge"
              checked={acknowledged}
              onChange={(event) => setAcknowledged(event.currentTarget.checked)}
            />
            <span>{t("settings.notificationDeliveryConsentAcknowledge")}</span>
          </label>
          <div className="session-actions">
            <button
              className="trust-action-button secondary"
              type="button"
              onClick={() => {
                setPending(null);
                setAcknowledged(false);
              }}
            >
              {t("action.cancel")}
            </button>
            <button
              className="trust-action-button primary"
              type="button"
              data-testid="notification-delivery-consent-accept"
              disabled={!acknowledged}
              onClick={accept}
            >
              {t("settings.notificationDeliveryConsentAccept")}
            </button>
          </div>
        </ModalDialog>
      ) : null}
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
      <h4 className="settings-subheading">{t("settings.notificationKeywordHeading")}</h4>
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

