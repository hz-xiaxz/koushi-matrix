import { useState } from "react";

import { type Locale, type MessageId, getActiveLocale, t } from "../../i18n/messages";
import type { AdvisoryDirectMessageSettlement } from "../../domain/types";
import { ModalDialog } from "../ModalDialog";

/**
 * #1494: the advisory dialog behind the sender-name ⓘ. It owns only its
 * visibility, the confirmation step and the selected message language; Core
 * decides whether a direct chat exists and whether the draft was seeded.
 */

export type AdvisoryLocale = Exclude<Locale, "pseudo">;

export const ADVISORY_LOCALES: readonly { locale: AdvisoryLocale; label: MessageId }[] = [
  { locale: "en", label: "settings.languageEnglish" },
  { locale: "ja", label: "settings.languageJapanese" },
  { locale: "zh-Hans", label: "settings.languageSimplifiedChinese" },
  { locale: "zh-Hant", label: "settings.languageTraditionalChinese" },
  { locale: "ko", label: "settings.languageKorean" }
];

function defaultAdvisoryLocale(): AdvisoryLocale {
  const active = getActiveLocale();
  return active === "pseudo" ? "en" : active;
}

export function advisoryText(name: string, locale: AdvisoryLocale): string {
  return t("dm.unverifiedDeviceAdvisory", { name }, locale);
}

type Phase = "advise" | "confirmCreate" | "keptExisting";

export function SenderDeviceAdvisoryDialog({
  senderDisplayLabel,
  onClose,
  onOpenUserInfo,
  onSendAdvisory
}: {
  senderDisplayLabel: string;
  onClose: () => void;
  onOpenUserInfo: () => void;
  /** Starts the advisory direct message; rejects on a command failure. */
  onSendAdvisory: (
    text: string,
    createIfMissing: boolean
  ) => Promise<AdvisoryDirectMessageSettlement>;
}) {
  const [locale, setLocale] = useState<AdvisoryLocale>(defaultAdvisoryLocale);
  const [phase, setPhase] = useState<Phase>("advise");
  const [isBusy, setBusy] = useState(false);
  const name = senderDisplayLabel;
  const text = advisoryText(name, locale);

  async function send(createIfMissing: boolean) {
    setBusy(true);
    try {
      const settlement = await onSendAdvisory(text, createIfMissing);
      if (settlement.status === "missing") {
        setPhase("confirmCreate");
      } else if (settlement.draft === "kept_existing") {
        setPhase("keptExisting");
      } else {
        onClose();
      }
    } catch {
      // Command failures are surfaced through the Rust-owned error path.
    } finally {
      setBusy(false);
    }
  }

  if (phase === "confirmCreate") {
    return (
      <ModalDialog
        title={t("people.unverifiedDevices.createDmTitle", { name })}
        className="confirmation-modal"
        showCloseButton={false}
        dismissible={!isBusy}
        onClose={onClose}
      >
        <div className="confirmation-content">
          <p>{t("people.unverifiedDevices.createDmBody", { name })}</p>
          <div className="dialog-actions">
            <button type="button" className="dialog-button" disabled={isBusy} onClick={onClose}>
              {t("action.cancel")}
            </button>
            <button
              type="button"
              className="dialog-button is-primary"
              disabled={isBusy}
              onClick={() => void send(true)}
            >
              {t("people.unverifiedDevices.createDmConfirm")}
            </button>
          </div>
        </div>
      </ModalDialog>
    );
  }

  if (phase === "keptExisting") {
    return (
      <ModalDialog
        title={t("people.unverifiedDevices.tooltipTitle")}
        className="confirmation-modal"
        showCloseButton={false}
        onClose={onClose}
      >
        <div className="confirmation-content">
          <p>{t("people.unverifiedDevices.keptExistingDraft")}</p>
          <div className="dialog-actions">
            <button type="button" className="dialog-button is-primary" onClick={onClose}>
              {t("action.done")}
            </button>
          </div>
        </div>
      </ModalDialog>
    );
  }

  return (
    <ModalDialog
      title={t("people.unverifiedDevices.tooltipTitle")}
      className="confirmation-modal sender-device-advisory-dialog"
      showCloseButton={false}
      dismissible={!isBusy}
      onClose={onClose}
    >
      <div className="confirmation-content">
        <p>{t("people.unverifiedDevices.tooltipBody", { name })}</p>
        <label className="profile-settings-field">
          <span>{t("people.unverifiedDevices.advisoryLanguage")}</span>
          <select
            value={locale}
            disabled={isBusy}
            onChange={(event) => setLocale(event.currentTarget.value as AdvisoryLocale)}
          >
            {ADVISORY_LOCALES.map((option) => (
              <option key={option.locale} value={option.locale}>
                {t(option.label)}
              </option>
            ))}
          </select>
        </label>
        <figure className="sender-device-advisory-preview">
          <figcaption>{t("people.unverifiedDevices.advisoryPreview")}</figcaption>
          <p dir="auto" lang={locale}>
            {text}
          </p>
        </figure>
        <div className="dialog-actions">
          <button type="button" className="dialog-button" disabled={isBusy} onClick={onClose}>
            {t("action.cancel")}
          </button>
          <button
            type="button"
            className="dialog-button"
            disabled={isBusy}
            onClick={() => {
              onClose();
              onOpenUserInfo();
            }}
          >
            {t("people.unverifiedDevices.openUserInfo")}
          </button>
          <button
            type="button"
            className="dialog-button is-primary"
            disabled={isBusy}
            onClick={() => void send(false)}
          >
            {t("people.unverifiedDevices.sendAdvisory")}
          </button>
        </div>
      </div>
    </ModalDialog>
  );
}
