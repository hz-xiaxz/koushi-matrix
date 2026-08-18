import { type FormEvent, useState } from "react";
import { Check, Edit3, RefreshCcw, X } from "lucide-react";

import { t } from "../../i18n/messages";
import { ImeSafeForm, ImeTextField } from "../ImeTextControl";
import { AccountManagementUiaForm } from "./AccountManagementUiaForm";
import type {
  AccountManagementState,
  DeviceSessionListState,
  DeviceSessionSummary
} from "../../domain/types";

export function SessionsSection({
  deviceSessions,
  accountManagement,
  onQueryDevices,
  onRenameDevice,
  onDeleteDevices,
  onSubmitAccountManagementUia
}: {
  deviceSessions: DeviceSessionListState;
  accountManagement: AccountManagementState;
  onQueryDevices: () => void;
  onRenameDevice: (deviceOrdinal: number, displayName: string) => void;
  onDeleteDevices: (deviceOrdinals: number[]) => void;
  onSubmitAccountManagementUia: (flowId: number, password: string) => void;
}) {
  const [renamingOrdinal, setRenamingOrdinal] = useState<number | null>(null);

  const currentDevice =
    deviceSessions.kind === "loaded"
      ? deviceSessions.devices.find((device) => device.current)
      : undefined;
  const otherDevices =
    deviceSessions.kind === "loaded"
      ? deviceSessions.devices.filter((device) => !device.current)
      : [];
  const otherOrdinals = otherDevices.map((device) => device.device_ordinal);

  return (
    <section id="settings-sessions" className="settings-section" aria-label={t("settings.sessions")}>
      <div className="settings-section-heading">
        <h3>{t("settings.sessions")}</h3>
      </div>

      {accountManagement.kind === "awaitingUia" &&
      (accountManagement.operation === "renameDevice" ||
        accountManagement.operation === "deleteDevice" ||
        accountManagement.operation === "deleteOtherDevices") ? (
        <AccountManagementUiaForm
          flowId={accountManagement.flow_id}
          onSubmit={onSubmitAccountManagementUia}
        />
      ) : null}

      {deviceSessions.kind === "idle" || deviceSessions.kind === "loading" ? (
        <p className="settings-status-text">{t("settings.sessionsLoading")}</p>
      ) : null}

      {deviceSessions.kind === "failed" ? (
        <>
          <p className="settings-status-text">{t("settings.sessionsLoadFailed")}</p>
          <button className="trust-action-button secondary" type="button" onClick={onQueryDevices}>
            <RefreshCcw size={14} />
            <span>{t("action.restartSync")}</span>
          </button>
        </>
      ) : null}

      <div className="sessions-list">
        {currentDevice ? (
          <div className="session-row session-row-current">
            <div className="session-main">
              <strong>{currentDevice.display_name ?? t("settings.deviceNamePlaceholder")}</strong>
              <span className="session-meta">{t("settings.currentSession")}</span>
            </div>
            <div className="session-badges">
              {currentDevice.verified ? (
                <span className="session-badge verified">{t("settings.deviceVerified")}</span>
              ) : (
                <span className="session-badge unverified">{t("settings.deviceUnverified")}</span>
              )}
              {currentDevice.inactive ? (
                <span className="session-badge inactive">{t("settings.deviceInactive")}</span>
              ) : null}
            </div>
          </div>
        ) : null}

        {otherDevices.length > 0 ? (
          <>
            <h4 className="settings-subheading">{t("settings.otherSessions")}</h4>
            {otherDevices.map((device) => (
              <SessionRow
                key={device.device_ordinal}
                device={device}
                renaming={renamingOrdinal === device.device_ordinal}
                onStartRename={() => setRenamingOrdinal(device.device_ordinal)}
                onCancelRename={() => setRenamingOrdinal(null)}
                onRename={(displayName) => {
                  setRenamingOrdinal(null);
                  onRenameDevice(device.device_ordinal, displayName);
                }}
                onSignOut={() => onDeleteDevices([device.device_ordinal])}
              />
            ))}
            <div className="session-actions">
              <button
                className="trust-action-button danger"
                type="button"
                onClick={() => onDeleteDevices(otherOrdinals)}
              >
                <X size={14} />
                <span>{t("settings.signOutOthers")}</span>
              </button>
            </div>
          </>
        ) : null}
      </div>
    </section>
  );
}

function SessionRow({
  device,
  renaming,
  onStartRename,
  onCancelRename,
  onRename,
  onSignOut
}: {
  device: DeviceSessionSummary;
  renaming: boolean;
  onStartRename: () => void;
  onCancelRename: () => void;
  onRename: (displayName: string) => void;
  onSignOut: () => void;
}) {
  const [draft, setDraft] = useState(device.display_name ?? "");

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = draft.trim();
    onRename(trimmed.length > 0 ? trimmed : device.display_name ?? "");
  }

  if (renaming) {
    return (
      <ImeSafeForm className="session-row session-row-renaming" onSubmit={submit}>
        <label className="session-rename-field">
          <span className="sr-only">{t("settings.deviceNamePlaceholder")}</span>
          <ImeTextField
            value={draft}
            syncKey={`session-rename:${device.device_ordinal}`}
            placeholder={t("settings.deviceNamePlaceholder")}
            onChange={(event) => setDraft(event.currentTarget.value)}
          />
        </label>
        <div className="session-actions">
          <button className="trust-action-button primary" type="submit">
            <Check size={14} />
            <span>{t("settings.renameDevice")}</span>
          </button>
          <button
            className="trust-action-button secondary"
            type="button"
            onClick={onCancelRename}
          >
            <X size={14} />
            <span>{t("action.cancel")}</span>
          </button>
        </div>
      </ImeSafeForm>
    );
  }

  return (
    <div className="session-row">
      <div className="session-main">
        <strong>{device.display_name ?? t("settings.deviceNamePlaceholder")}</strong>
      </div>
      <div className="session-badges">
        {device.verified ? (
          <span className="session-badge verified">{t("settings.deviceVerified")}</span>
        ) : (
          <span className="session-badge unverified">{t("settings.deviceUnverified")}</span>
        )}
        {device.inactive ? (
          <span className="session-badge inactive">{t("settings.deviceInactive")}</span>
        ) : null}
      </div>
      <div className="session-actions">
        <button className="trust-action-button secondary" type="button" onClick={onStartRename}>
          <Edit3 size={14} />
          <span>{t("settings.renameDevice")}</span>
        </button>
        <button className="trust-action-button danger" type="button" onClick={onSignOut}>
          <X size={14} />
          <span>{t("settings.signOut")}</span>
        </button>
      </div>
    </div>
  );
}
