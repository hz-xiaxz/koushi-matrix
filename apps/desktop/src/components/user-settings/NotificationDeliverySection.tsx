import { useMemo, useRef, useState } from "react";
import { Bell, KeyRound, Plus, Send, Trash2 } from "lucide-react";

import { t } from "../../i18n/messages";
import { ImeTextField, SecureImeTextField } from "../ImeTextControl";
import { ModalDialog } from "../ModalDialog";
import { roomDisplayLabel } from "../../domain/roomDisplayLabel";
import {
  DEFAULT_NOTIFICATION_DELIVERY_PREFIX_CHARS,
  MAX_NOTIFICATION_DELIVERY_SINKS,
  MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS,
  clampNotificationDeliveryPrefixChars,
  clampNotificationDeliverySeconds,
  createNotificationSinkId,
  emptyNotificationSinkDraft,
  notificationDeliveryOutcomeMessage,
  notificationDeliveryPrefixConsented,
  notificationDeliveryServiceLabel,
  notificationDeliverySinkFailed,
  notificationDraftReady,
  notificationSinkEndpoint,
  notificationSinkFromDraft,
  notificationSmtpConfigFromPreset,
  notificationTargetsForLabel,
  type NotificationSinkDraft
} from "../../domain/notificationDelivery";
import type {
  NotificationDeliveryPresetState,
  NotificationDeliverySettings,
  NotificationDeliverySinkState,
  NotificationDeliveryState,
  NotificationDeliveryTarget,
  NotificationSinkSettings,
  RoomSummary,
  SmtpTls,
  SpaceSummary
} from "../../domain/types";

/**
 * Typed commands of the device-local notification delivery section (#1427).
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
 * Settings -> Notifications, "Notify me from this device".
 *
 * Everything shown is Rust state: `delivery` is the settings block, `state` is the
 * app-scoped runtime half (whether a credential exists, and the last outcome per sink), and
 * `rooms`/`spaces` supply the labels the watch list renders. This component computes no
 * dwell, starts no timer, assembles no message, and never reads a secret back.
 */
export function NotificationDeliverySection({
  delivery,
  state,
  rooms = [],
  spaces = [],
  actions
}: {
  delivery: NotificationDeliverySettings;
  state: NotificationDeliveryState;
  rooms?: RoomSummary[];
  spaces?: SpaceSummary[];
  actions: NotificationDeliveryActions;
}) {
  const [addOpen, setAddOpen] = useState(false);
  const [draft, setDraft] = useState<NotificationSinkDraft>(emptyNotificationSinkDraft);
  const [consentSinkId, setConsentSinkId] = useState<string | null>(null);
  const [consentAcknowledged, setConsentAcknowledged] = useState(false);
  const [busySinkId, setBusySinkId] = useState<string | null>(null);

  const sinkStates = useMemo(() => {
    const map = new Map<string, NotificationDeliverySinkState>();
    for (const sinkState of state.sinks) {
      map.set(sinkState.sink_id, sinkState);
    }
    return map;
  }, [state.sinks]);

  const watched = notificationTargetsForLabel({ watched: delivery.watched, rooms, spaces });
  const watchedKeys = new Set(watched.map((target) => `${target.kind}:${target.id}`));
  const addableRooms = rooms.filter((room) => !watchedKeys.has(`room:${room.room_id}`));
  const addableSpaces = spaces.filter((space) => !watchedKeys.has(`space:${space.space_id}`));

  function update(delivery: NotificationDeliverySettings) {
    actions.update(delivery);
  }

  function replaceSink(next: NotificationSinkSettings) {
    update({
      ...delivery,
      sinks: delivery.sinks.map((sink) => (sink.id === next.id ? next : sink))
    });
  }

  function removeSink(sinkId: string) {
    // Removing the sink also releases the credential it was keyed by; leaving it behind
    // would keep an orphaned secret in the OS store.
    void actions.clearSecret(sinkId);
    update({ ...delivery, sinks: delivery.sinks.filter((sink) => sink.id !== sinkId) });
  }

  function addWatched(target: NotificationDeliveryTarget) {
    update({ ...delivery, watched: [...delivery.watched, target] });
  }

  function removeWatched(target: NotificationDeliveryTarget) {
    update({
      ...delivery,
      watched: delivery.watched.filter((candidate) =>
        candidate.kind === "room" && target.kind === "room"
          ? candidate.room_id !== target.room_id
          : candidate.kind === "space" && target.kind === "space"
            ? candidate.space_id !== target.space_id
            : true
      )
    });
  }

  function confirmAddSink() {
    if (!notificationDraftReady(draft)) {
      return;
    }
    update({ ...delivery, sinks: [...delivery.sinks, notificationSinkFromDraft(createNotificationSinkId(), draft)] });
    setDraft(emptyNotificationSinkDraft());
    setAddOpen(false);
  }

  function acceptPrefixConsent(sink: NotificationSinkSettings) {
    const endpoint = notificationSinkEndpoint(sink);
    update({
      ...delivery,
      prefix_chars:
        delivery.prefix_chars > 0
          ? delivery.prefix_chars
          : DEFAULT_NOTIFICATION_DELIVERY_PREFIX_CHARS,
      // The consent names the exact sink, destination and endpoint it was given for, so a
      // later change to any of them is a new question rather than an inherited answer.
      prefix_consent: {
        sink_id: sink.id,
        destination: sink.destination,
        endpoint,
        accepted_at_ms: Date.now()
      }
    });
    setConsentSinkId(null);
    setConsentAcknowledged(false);
  }

  function turnPrefixOff() {
    update({ ...delivery, prefix_chars: 0, prefix_consent: null });
  }

  const consentSink = consentSinkId
    ? delivery.sinks.find((sink) => sink.id === consentSinkId) ?? null
    : null;

  return (
    <>
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

        <div className="settings-toggle-list">
          <ToggleRow
            label={t("settings.notificationDeliveryEnable")}
            testId="notification-delivery-enabled"
            checked={delivery.enabled}
            onToggle={(enabled) => update({ ...delivery, enabled })}
          />
        </div>

        <h4 className="settings-subheading">{t("settings.notificationDeliveryTriggers")}</h4>
        <div className="settings-toggle-list">
          <ToggleRow
            label={t("settings.notificationDeliveryDirectMessages")}
            testId="notification-delivery-direct-messages"
            checked={delivery.direct_messages}
            onToggle={(direct_messages) => update({ ...delivery, direct_messages })}
          />
          <ToggleRow
            label={t("settings.notificationDeliveryMentions")}
            testId="notification-delivery-mentions"
            checked={delivery.mentions}
            onToggle={(mentions) => update({ ...delivery, mentions })}
          />
          <ToggleRow
            label={t("settings.notificationDeliveryKeywords")}
            testId="notification-delivery-keywords"
            checked={delivery.keywords}
            onToggle={(keywords) => update({ ...delivery, keywords })}
          />
        </div>
        <p className="profile-settings-hint" data-testid="notification-delivery-keywords-hint">
          {t("settings.notificationDeliveryKeywordsHint")}
        </p>

        <div className="settings-toggle-list">
          <ToggleRow
            label={t("settings.notificationDeliveryOnlyUnfocused")}
            testId="notification-delivery-only-unfocused"
            checked={delivery.only_when_unfocused}
            onToggle={(only_when_unfocused) => update({ ...delivery, only_when_unfocused })}
          />
        </div>

        <div className="settings-control-stack">
          <label className="profile-settings-field">
            <span>{t("settings.notificationDeliveryDwell")}</span>
            <ImeTextField
              type="text"
              inputMode="numeric"
              maxLength={6}
              value={String(delivery.dwell_secs)}
              // The key names the committed value: Rust is what bounds the field, so a
              // rejected entry has to be replaced by the bound the settings now hold.
              syncKey={`notification-delivery-dwell:${delivery.dwell_secs}`}
              data-testid="notification-delivery-dwell"
              onBlur={(event) =>
                update({
                  ...delivery,
                  dwell_secs: clampNotificationDeliverySeconds(Number(event.currentTarget.value))
                })
              }
            />
          </label>
          <p className="profile-settings-hint">{t("settings.notificationDeliveryDwellDescription")}</p>
          <label className="profile-settings-field">
            <span>{t("settings.notificationDeliveryInterval")}</span>
            <ImeTextField
              type="text"
              inputMode="numeric"
              maxLength={6}
              value={String(delivery.min_interval_secs)}
              syncKey={`notification-delivery-interval:${delivery.min_interval_secs}`}
              data-testid="notification-delivery-interval"
              onBlur={(event) =>
                update({
                  ...delivery,
                  min_interval_secs: clampNotificationDeliverySeconds(Number(event.currentTarget.value))
                })
              }
            />
          </label>
          <p className="profile-settings-hint">
            {t("settings.notificationDeliveryIntervalDescription")}
          </p>
        </div>

        <section
          className="settings-section"
          aria-label={t("settings.notificationDeliveryWatched")}
        >
          <h4 className="settings-subheading">{t("settings.notificationDeliveryWatched")}</h4>
          <p className="profile-settings-hint">{t("settings.notificationDeliveryWatchedDescription")}</p>
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
                      onClick={() =>
                        removeWatched(
                          target.kind === "room"
                            ? { kind: "room", room_id: target.id }
                            : { kind: "space", space_id: target.id }
                        )
                      }
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
              disabled={delivery.watched.length >= MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS}
              value=""
              onChange={(event) => {
                const [kind, ...rest] = event.currentTarget.value.split(":");
                const id = rest.join(":");
                if (!id) {
                  return;
                }
                addWatched(
                  kind === "space" ? { kind: "space", space_id: id } : { kind: "room", room_id: id }
                );
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
          {delivery.watched.length >= MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS ? (
            <p className="profile-settings-hint" data-testid="notification-delivery-watched-limit">
              {t("settings.notificationDeliveryWatchedLimit", {
                count: MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS
              })}
            </p>
          ) : null}
        </section>

        <section
          className="settings-section"
          aria-label={t("settings.notificationDeliverySinks")}
        >
          <div className="settings-section-heading">
            <div>
              <h4 className="settings-subheading">{t("settings.notificationDeliverySinks")}</h4>
            </div>
          </div>
          {delivery.sinks.length === 0 ? (
            <p className="settings-status-text" data-testid="notification-delivery-sinks-empty">
              {t("settings.notificationDeliverySinkEmpty")}
            </p>
          ) : (
            <div className="settings-detail-list" data-testid="notification-delivery-sinks">
              {delivery.sinks.map((sink) => (
                <NotificationSinkCard
                  key={sink.id}
                  sink={sink}
                  sinkState={sinkStates.get(sink.id) ?? null}
                  presets={state.presets}
                  prefixConsented={notificationDeliveryPrefixConsented(delivery, sink)}
                  prefixChars={delivery.prefix_chars}
                  busy={busySinkId === sink.id}
                  onUpdate={replaceSink}
                  onRemove={() => removeSink(sink.id)}
                  onSetSecret={async (secret) => {
                    setBusySinkId(sink.id);
                    try {
                      await actions.setSecret(sink.id, secret);
                    } finally {
                      setBusySinkId(null);
                    }
                  }}
                  onClearSecret={() => {
                    void actions.clearSecret(sink.id);
                  }}
                  onTest={async () => {
                    setBusySinkId(sink.id);
                    try {
                      await actions.test(sink.id);
                    } finally {
                      setBusySinkId(null);
                    }
                  }}
                  onRequestPrefixConsent={() => {
                    setConsentAcknowledged(false);
                    setConsentSinkId(sink.id);
                  }}
                  onTurnPrefixOff={turnPrefixOff}
                  onPrefixCharsChange={(chars) =>
                    update({ ...delivery, prefix_chars: clampNotificationDeliveryPrefixChars(chars) })
                  }
                />
              ))}
            </div>
          )}

          {addOpen ? (
            <div className="profile-settings-form" data-testid="notification-delivery-add-form">
              <label className="profile-settings-field">
                <span>{t("settings.notificationDeliverySinkProvider")}</span>
                <select
                  data-testid="notification-delivery-add-kind"
                  value={draft.kind}
                  onChange={(event) =>
                    setDraft({ ...draft, kind: event.currentTarget.value === "smtp" ? "smtp" : "resend" })
                  }
                >
                  <option value="resend">{t("settings.notificationDeliverySinkKindResend")}</option>
                  <option value="smtp">{t("settings.notificationDeliverySinkKindSmtp")}</option>
                </select>
              </label>
              <label className="profile-settings-field">
                <span>{t("settings.notificationDeliverySinkFrom")}</span>
                <ImeTextField
                  type="email"
                  value={draft.from_address}
                  syncKey="notification-delivery-add-from"
                  data-testid="notification-delivery-add-from"
                  onChange={(event) => setDraft({ ...draft, from_address: event.currentTarget.value })}
                />
              </label>
              <label className="profile-settings-field">
                <span>{t("settings.notificationDeliverySinkName")}</span>
                <ImeTextField
                  value={draft.sender_name}
                  syncKey="notification-delivery-add-name"
                  data-testid="notification-delivery-add-name"
                  onChange={(event) => setDraft({ ...draft, sender_name: event.currentTarget.value })}
                />
              </label>
              <label className="profile-settings-field">
                <span>{t("settings.notificationDeliverySinkTo")}</span>
                <ImeTextField
                  type="email"
                  value={draft.destination}
                  syncKey="notification-delivery-add-to"
                  data-testid="notification-delivery-add-to"
                  onChange={(event) => setDraft({ ...draft, destination: event.currentTarget.value })}
                />
              </label>
              {draft.kind === "resend" ? (
                <label className="profile-settings-field">
                  <span>{t("settings.notificationDeliverySinkEndpoint")}</span>
                  <ImeTextField
                    type="url"
                    value={draft.endpoint}
                    syncKey="notification-delivery-add-endpoint"
                    data-testid="notification-delivery-add-endpoint"
                    onChange={(event) => setDraft({ ...draft, endpoint: event.currentTarget.value })}
                  />
                  <span className="profile-settings-hint">
                    {t("settings.notificationDeliverySinkEndpointDefault")}
                  </span>
                </label>
              ) : (
                <>
                  <label className="profile-settings-field">
                    <span>{t("settings.notificationDeliverySinkPreset")}</span>
                    <select
                      data-testid="notification-delivery-add-preset"
                      value={draft.preset ?? ""}
                      onChange={(event) => {
                        const preset = state.presets.find(
                          (candidate) => candidate.id === event.currentTarget.value
                        );
                        if (!preset) {
                          setDraft({ ...draft, preset: null });
                          return;
                        }
                        const presetConfig = notificationSmtpConfigFromPreset(
                          preset,
                          draft.from_address
                        );
                        setDraft({
                          ...draft,
                          preset: preset.id,
                          host: presetConfig.host,
                          port: presetConfig.port,
                          tls: presetConfig.tls,
                          username: presetConfig.username
                        });
                      }}
                    >
                      <option value="">{t("settings.notificationDeliverySinkPresetCustom")}</option>
                      {state.presets.map((preset) => (
                        <option key={preset.id} value={preset.id}>
                          {preset.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <p className="profile-settings-hint">
                    {t("settings.notificationDeliverySinkPresetHint")}
                  </p>
                  <label className="profile-settings-field">
                    <span>{t("settings.notificationDeliverySinkHost")}</span>
                    <ImeTextField
                      value={draft.host}
                      syncKey="notification-delivery-add-host"
                      data-testid="notification-delivery-add-host"
                      onChange={(event) => setDraft({ ...draft, host: event.currentTarget.value })}
                    />
                  </label>
                  <label className="profile-settings-field">
                    <span>{t("settings.notificationDeliverySinkPort")}</span>
                    <ImeTextField
                      type="text"
                      inputMode="numeric"
                      maxLength={5}
                      value={draft.port > 0 ? String(draft.port) : ""}
                      syncKey={`notification-delivery-add-port:${draft.port}`}
                      data-testid="notification-delivery-add-port"
                      onBlur={(event) =>
                        setDraft({ ...draft, port: smtpPort(Number(event.currentTarget.value)) })
                      }
                    />
                  </label>
                  <label className="profile-settings-field">
                    <span>{t("settings.notificationDeliverySinkTls")}</span>
                    <select
                      data-testid="notification-delivery-add-tls"
                      value={draft.tls}
                      onChange={(event) =>
                        setDraft({ ...draft, tls: event.currentTarget.value as SmtpTls })
                      }
                    >
                      <option value="implicit">
                        {t("settings.notificationDeliverySinkTlsImplicit")}
                      </option>
                      <option value="startTls">
                        {t("settings.notificationDeliverySinkTlsStartTls")}
                      </option>
                    </select>
                  </label>
                  <label className="profile-settings-field">
                    <span>{t("settings.notificationDeliverySinkUsername")}</span>
                    <ImeTextField
                      value={draft.username}
                      syncKey="notification-delivery-add-username"
                      data-testid="notification-delivery-add-username"
                      placeholder={
                        selectedPresetIsFullAddress(state.presets, draft.preset)
                          ? t("settings.notificationDeliverySinkUsernameFullAddress")
                          : undefined
                      }
                      onChange={(event) => setDraft({ ...draft, username: event.currentTarget.value })}
                    />
                  </label>
                  <SecretHint presets={state.presets} presetId={draft.preset} />
                </>
              )}
              <div className="session-actions">
                <button
                  className="trust-action-button secondary"
                  type="button"
                  onClick={() => {
                    setDraft(emptyNotificationSinkDraft());
                    setAddOpen(false);
                  }}
                >
                  {t("action.cancel")}
                </button>
                <button
                  className="trust-action-button primary"
                  type="button"
                  data-testid="notification-delivery-add-confirm"
                  disabled={!notificationDraftReady(draft)}
                  onClick={confirmAddSink}
                >
                  {t("action.add")}
                </button>
              </div>
            </div>
          ) : (
            <div className="session-actions">
              <button
                className="trust-action-button secondary"
                type="button"
                data-testid="notification-delivery-add"
                disabled={delivery.sinks.length >= MAX_NOTIFICATION_DELIVERY_SINKS}
                onClick={() => setAddOpen(true)}
              >
                <Plus size={14} aria-hidden="true" />
                <span>{t("settings.notificationDeliveryAddSink")}</span>
              </button>
            </div>
          )}
          {delivery.sinks.length >= MAX_NOTIFICATION_DELIVERY_SINKS ? (
            <p className="profile-settings-hint" data-testid="notification-delivery-sinks-limit">
              {t("settings.notificationDeliverySinkLimit", {
                count: MAX_NOTIFICATION_DELIVERY_SINKS
              })}
            </p>
          ) : null}
        </section>
      </section>

      {consentSink ? (
        <ModalDialog
          title={t("settings.notificationDeliveryPrefixConsentTitle")}
          className="dialog-box"
          onClose={() => {
            setConsentSinkId(null);
            setConsentAcknowledged(false);
          }}
        >
          <p className="profile-settings-hint" data-testid="notification-delivery-consent-body">
            {t("settings.notificationDeliveryPrefixConsentBody", {
              service: notificationDeliveryServiceLabel(consentSink, state.presets),
              destination: consentSink.destination
            })}
          </p>
          <p className="profile-settings-hint">
            {t("settings.notificationDeliveryPrefixConsentLink")}
          </p>
          <label className="settings-detail-row">
            <input
              type="checkbox"
              data-testid="notification-delivery-consent-acknowledge"
              checked={consentAcknowledged}
              onChange={(event) => setConsentAcknowledged(event.currentTarget.checked)}
            />
            <span>{t("settings.notificationDeliveryPrefixConsentAcknowledge")}</span>
          </label>
          <p className="profile-settings-hint">
            {t("settings.notificationDeliveryPrefixConsentRequired")}
          </p>
          <div className="session-actions">
            <button
              className="trust-action-button secondary"
              type="button"
              onClick={() => {
                setConsentSinkId(null);
                setConsentAcknowledged(false);
              }}
            >
              {t("action.cancel")}
            </button>
            <button
              className="trust-action-button primary"
              type="button"
              data-testid="notification-delivery-consent-accept"
              disabled={!consentAcknowledged}
              onClick={() => acceptPrefixConsent(consentSink)}
            >
              {t("settings.notificationDeliveryPrefixConsentAccept")}
            </button>
          </div>
        </ModalDialog>
      ) : null}
    </>
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

function NotificationSinkCard({
  sink,
  sinkState,
  presets,
  prefixConsented,
  prefixChars,
  busy,
  onUpdate,
  onRemove,
  onSetSecret,
  onClearSecret,
  onTest,
  onRequestPrefixConsent,
  onTurnPrefixOff,
  onPrefixCharsChange
}: {
  sink: NotificationSinkSettings;
  sinkState: NotificationDeliverySinkState | null;
  presets: readonly NotificationDeliveryPresetState[];
  prefixConsented: boolean;
  prefixChars: number;
  busy: boolean;
  onUpdate: (sink: NotificationSinkSettings) => void;
  onRemove: () => void;
  onSetSecret: (secret: string) => Promise<void>;
  onClearSecret: () => void;
  onTest: () => Promise<void>;
  onRequestPrefixConsent: () => void;
  onTurnPrefixOff: () => void;
  onPrefixCharsChange: (chars: number) => void;
}) {
  // The secret is read from the DOM at the explicit save/cancel boundary and cleared there;
  // the value never enters React state and is never written back from the snapshot.
  const secretRef = useRef<HTMLInputElement | null>(null);
  // Held as a local const so the discriminated union stays narrowed inside the handlers.
  const config = sink.config;
  const outcome = sinkState?.last_outcome ?? null;
  const outcomeMessage = notificationDeliveryOutcomeMessage(outcome);
  const failed = notificationDeliverySinkFailed(outcome);
  const service = notificationDeliveryServiceLabel(sink, presets);
  const secretConfigured = Boolean(sinkState?.secret_configured);

  /**
   * One field edit becomes one whole-block patch, built from the sink Rust last reported, so
   * the section never merges a partial delivery state by hand.
   */
  function commitText(
    field: "destination" | "from_address" | "sender_name" | "endpoint" | "host" | "username",
    value: string
  ) {
    const trimmed = value.trim();
    if (field === "endpoint" || field === "host" || field === "username") {
      if (config.kind === "resend") {
        onUpdate({ ...sink, config: { ...config, endpoint: trimmed } });
        return;
      }
      if (field === "host") {
        onUpdate({ ...sink, config: { ...config, host: trimmed } });
        return;
      }
      if (field === "username") {
        onUpdate({ ...sink, config: { ...config, username: trimmed } });
        return;
      }
      return;
    }
    if (field === "destination") {
      onUpdate({ ...sink, destination: trimmed });
      return;
    }
    if (field === "from_address") {
      onUpdate({ ...sink, from_address: trimmed });
      return;
    }
    onUpdate({ ...sink, sender_name: trimmed });
  }

  function commitPort(value: string) {
    if (config.kind !== "smtp") {
      return;
    }
    onUpdate({ ...sink, config: { ...config, port: smtpPort(Number(value)) } });
  }

  return (
    <article
      className="settings-detail-row"
      data-testid="notification-delivery-sink"
      data-sink-id={sink.id}
      data-state={failed ? "failed" : "ok"}
    >
      <button
        className="settings-toggle-row"
        type="button"
        role="switch"
        aria-checked={sink.enabled}
        aria-label={`${service}: ${t("settings.notificationDeliverySinkEnabled")}`}
        data-testid="notification-delivery-sink-enabled"
        onClick={() => onUpdate({ ...sink, enabled: !sink.enabled })}
      >
        <span className="settings-toggle-copy">
          <span className="settings-toggle-label">
            <Send size={15} aria-hidden="true" />
            <span dir="auto">{service}</span>
          </span>
          {failed ? (
            // Rust's `enabled` stays on the switch; the row says plainly that the sends are
            // not arriving, so an enabled sink is never read as a working one.
            <span
              className="settings-toggle-description"
              data-testid="notification-delivery-sink-failed"
            >
              {t("settings.notificationDeliverySinkFailed")}
            </span>
          ) : null}
        </span>
        <span className="settings-switch-track" aria-hidden="true">
          <span className="settings-switch-thumb" />
        </span>
      </button>

      <p className="settings-status-text" data-testid="notification-delivery-sink-outcome">
        {t("settings.notificationDeliveryLastResult")}:{" "}
        {outcomeMessage
          ? t(outcomeMessage.id, outcomeMessage.values)
          : t("settings.notificationDeliveryNeverSent")}
      </p>

      <label className="profile-settings-field">
        <span>{t("settings.notificationDeliverySinkFrom")}</span>
        <ImeTextField
          type="email"
          value={sink.from_address}
          syncKey={`sink-from:${sink.id}:${sink.from_address}`}
          data-testid="notification-delivery-sink-from"
          onBlur={(event) => commitText("from_address", event.currentTarget.value)}
        />
      </label>
      <label className="profile-settings-field">
        <span>{t("settings.notificationDeliverySinkName")}</span>
        <ImeTextField
          value={sink.sender_name}
          syncKey={`sink-name:${sink.id}:${sink.sender_name}`}
          data-testid="notification-delivery-sink-name"
          onBlur={(event) => commitText("sender_name", event.currentTarget.value)}
        />
      </label>
      <label className="profile-settings-field">
        <span>{t("settings.notificationDeliverySinkTo")}</span>
        <ImeTextField
          type="email"
          value={sink.destination}
          syncKey={`sink-to:${sink.id}:${sink.destination}`}
          data-testid="notification-delivery-sink-to"
          onBlur={(event) => commitText("destination", event.currentTarget.value)}
        />
      </label>

      {config.kind === "resend" ? (
        <label className="profile-settings-field">
          <span>{t("settings.notificationDeliverySinkEndpoint")}</span>
          <ImeTextField
            type="url"
            value={config.endpoint}
            syncKey={`sink-endpoint:${sink.id}:${config.endpoint}`}
            data-testid="notification-delivery-sink-endpoint"
            onBlur={(event) => commitText("endpoint", event.currentTarget.value)}
          />
          <span className="profile-settings-hint">
            {t("settings.notificationDeliverySinkEndpointDefault")}
          </span>
        </label>
      ) : (
        <>
          <label className="profile-settings-field">
            <span>{t("settings.notificationDeliverySinkPreset")}</span>
            <select
              data-testid="notification-delivery-sink-preset"
              value={config.preset ?? ""}
              onChange={(event) => {
                const preset = presets.find((candidate) => candidate.id === event.currentTarget.value);
                if (!preset) {
                  onUpdate({ ...sink, config: { ...config, preset: null } });
                  return;
                }
                const presetConfig = notificationSmtpConfigFromPreset(preset, sink.from_address);
                onUpdate({ ...sink, config: { ...presetConfig } });
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
          <label className="profile-settings-field">
            <span>{t("settings.notificationDeliverySinkHost")}</span>
            <ImeTextField
              value={config.host}
              syncKey={`sink-host:${sink.id}:${config.host}`}
              data-testid="notification-delivery-sink-host"
              onBlur={(event) => commitText("host", event.currentTarget.value)}
            />
          </label>
          <label className="profile-settings-field">
            <span>{t("settings.notificationDeliverySinkPort")}</span>
            <ImeTextField
              type="text"
              inputMode="numeric"
              maxLength={5}
              value={String(config.port)}
              syncKey={`sink-port:${sink.id}:${config.port}`}
              data-testid="notification-delivery-sink-port"
              onBlur={(event) => commitPort(event.currentTarget.value)}
            />
          </label>
          <label className="profile-settings-field">
            <span>{t("settings.notificationDeliverySinkTls")}</span>
            <select
              data-testid="notification-delivery-sink-tls"
              value={config.tls}
              onChange={(event) =>
                onUpdate({
                  ...sink,
                  config: { ...config, tls: event.currentTarget.value as SmtpTls }
                })
              }
            >
              <option value="implicit">{t("settings.notificationDeliverySinkTlsImplicit")}</option>
              <option value="startTls">{t("settings.notificationDeliverySinkTlsStartTls")}</option>
            </select>
          </label>
          <label className="profile-settings-field">
            <span>{t("settings.notificationDeliverySinkUsername")}</span>
            <ImeTextField
              value={config.username}
              syncKey={`sink-username:${sink.id}:${config.username}`}
              data-testid="notification-delivery-sink-username"
              placeholder={
                selectedPresetIsFullAddress(presets, config.preset ?? null)
                  ? t("settings.notificationDeliverySinkUsernameFullAddress")
                  : undefined
              }
              onBlur={(event) => commitText("username", event.currentTarget.value)}
            />
          </label>
          <SecretHint presets={presets} presetId={config.preset ?? null} />
        </>
      )}

      <label className="profile-settings-field">
        <span>{t("settings.notificationDeliverySinkSecret")}</span>
        <SecureImeTextField
          ref={secretRef}
          autoComplete="off"
          syncKey={`sink-secret:${sink.id}`}
          data-testid="notification-delivery-sink-secret"
          aria-label={t("settings.notificationDeliverySinkSecret")}
        />
      </label>
      <p className="profile-settings-hint" data-testid="notification-delivery-sink-secret-state">
        {secretConfigured
          ? t("settings.notificationDeliverySinkSecretSet")
          : t("settings.notificationDeliverySinkSecretMissing")}
      </p>
      <div className="session-actions">
        <button
          className="trust-action-button secondary"
          type="button"
          data-testid="notification-delivery-sink-save-secret"
          disabled={busy}
          onClick={() => {
            const field = secretRef.current;
            if (!field) {
              return;
            }
            const secret = field.value;
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

      <div className="session-actions">
        <button
          className="trust-action-button secondary"
          type="button"
          data-testid="notification-delivery-sink-test"
          disabled={busy}
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

      <div className="settings-toggle-list">
        <button
          className="settings-toggle-row"
          type="button"
          role="switch"
          aria-checked={prefixConsented}
          aria-label={t("settings.notificationDeliveryPrefix")}
          data-testid="notification-delivery-sink-prefix"
          onClick={() => (prefixConsented ? onTurnPrefixOff() : onRequestPrefixConsent())}
        >
          <span className="settings-toggle-copy">
            <span className="settings-toggle-label">
              <span>{t("settings.notificationDeliveryPrefix")}</span>
            </span>
            <span className="settings-toggle-description">
              {t("settings.notificationDeliveryPrefixDescription")}
            </span>
          </span>
          <span className="settings-switch-track" aria-hidden="true">
            <span className="settings-switch-thumb" />
          </span>
        </button>
      </div>
      {prefixConsented ? (
        <label className="profile-settings-field">
          <span>{t("settings.notificationDeliveryPrefixChars")}</span>
          <ImeTextField
            type="text"
            inputMode="numeric"
            maxLength={3}
            value={String(prefixChars)}
            syncKey={`sink-prefix-chars:${sink.id}:${prefixChars}`}
            data-testid="notification-delivery-sink-prefix-chars"
            onBlur={(event) => onPrefixCharsChange(Number(event.currentTarget.value))}
          />
        </label>
      ) : prefixChars > 0 ? (
        <p className="profile-settings-hint" data-testid="notification-delivery-sink-prefix-required">
          {t("settings.notificationDeliveryPrefixConsentRequired")}
        </p>
      ) : null}

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
    </article>
  );
}
