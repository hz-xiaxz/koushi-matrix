import { useId, useState } from "react";
import { t } from "../i18n/messages";
import type { RecoveryKeyDeliveryState } from "../domain/types";

/**
 * Writes the revealed recovery key to the system clipboard through the same
 * `navigator.clipboard` path the rest of the app uses for copy actions.
 */
export async function copyRecoveryKeyToClipboard(recoveryKey: string): Promise<void> {
  if (!navigator.clipboard) throw new Error("clipboard unavailable");
  await navigator.clipboard.writeText(recoveryKey);
}

type CopyStatus = "idle" | "copied" | "failed";

/**
 * On-screen recovery-key reveal shared by the Secure Backup gate, Settings
 * setup, and passphrase change (#927).
 *
 * The key is rendered straight from the Rust snapshot prop and is never
 * copied into React state, refs, storage, or diagnostics; it disappears as
 * soon as Rust drops it on confirmation, session change, or account switch.
 * Copy and "Save to file" never advance the flow: only the explicit
 * confirmation does.
 */
export function RecoveryKeyReveal({
  recoveryKey,
  delivery,
  copyRecoveryKey = copyRecoveryKeyToClipboard,
  onSaveToFile,
  onConfirmSaved,
  confirmationFailed = false,
  disabled = false
}: {
  recoveryKey: string;
  delivery: RecoveryKeyDeliveryState;
  copyRecoveryKey?: (recoveryKey: string) => Promise<void>;
  onSaveToFile?: () => Promise<void>;
  onConfirmSaved: () => Promise<void>;
  confirmationFailed?: boolean;
  disabled?: boolean;
}) {
  const titleId = useId();
  const [copyStatus, setCopyStatus] = useState<CopyStatus>("idle");
  const [saving, setSaving] = useState(false);
  const [confirming, setConfirming] = useState(false);

  const copy = async () => {
    try {
      await copyRecoveryKey(recoveryKey);
      setCopyStatus("copied");
    } catch {
      setCopyStatus("failed");
    }
  };
  const save = async () => {
    if (!onSaveToFile || saving) return;
    setSaving(true);
    try {
      await onSaveToFile();
    } finally {
      setSaving(false);
    }
  };
  const confirm = async () => {
    if (confirming) return;
    setConfirming(true);
    try {
      await onConfirmSaved();
    } finally {
      setConfirming(false);
    }
  };

  return (
    <section className="recovery-key-reveal" role="region" aria-labelledby={titleId}>
      <h2 id={titleId}>{t("gate.secureBackupRecoveryKeyTitle")}</h2>
      <p>{t("gate.secureBackupRecoveryKeyCopy")}</p>
      <code className="recovery-key-value">{recoveryKey}</code>
      <div className="dialog-actions">
        <button className="dialog-button" type="button" onClick={() => void copy()}>
          {t("gate.secureBackupCopyRecoveryKey")}
        </button>
        {onSaveToFile && (
          <button
            className="dialog-button"
            type="button"
            disabled={saving}
            onClick={() => void save()}
          >
            {t("gate.secureBackupSaveRecoveryKeyToFile")}
          </button>
        )}
      </div>
      {copyStatus === "copied" && <p role="status">{t("gate.secureBackupRecoveryKeyCopied")}</p>}
      {copyStatus === "failed" && (
        <p role="alert">{t("gate.secureBackupRecoveryKeyCopyFailed")}</p>
      )}
      {delivery.kind === "written" && (
        <p role="status">{t("gate.secureBackupRecoveryKeySavedToFile")}</p>
      )}
      {delivery.kind === "writeFailed" && (
        <p role="alert">{t("gate.secureBackupRecoveryKeySaveFailed")}</p>
      )}
      {confirmationFailed && (
        <p role="alert">{t("gate.secureBackupRecoveryKeyConfirmFailed")}</p>
      )}
      <button
        className="dialog-button is-primary"
        type="button"
        disabled={disabled || confirming}
        onClick={() => void confirm()}
      >
        {t("gate.saved")}
      </button>
    </section>
  );
}
