import { KeyRound } from "lucide-react";
import type { HistoryKeyRecoveryFailure, HistoryKeyRecoveryState } from "../domain/types";
import { t, type MessageId } from "../i18n/messages";

const failureMessages: Record<HistoryKeyRecoveryFailure, MessageId> = {
  verificationRequired: "historyKeys.verify", network: "historyKeys.network",
  membershipChanged: "historyKeys.membership", historyNotShared: "historyKeys.visibility",
  tooLarge: "historyKeys.limit", noRecipients: "historyKeys.noRecipients",
  rateLimited: "historyKeys.rateLimit", storage: "historyKeys.storage", invalidResponse: "historyKeys.invalid"
};

export function HistoryKeyRecoveryControl({ state, onRequest, forceAll = false }: {
  state?: HistoryKeyRecoveryState;
  onRequest: () => void;
  forceAll?: boolean;
}) {
  const status = state?.status;
  const requested = status?.kind === "requested" || status?.kind === "partial";
  return <div className="history-key-recovery">
    {requested ? <p role="status" aria-live="polite" className="profile-settings-hint">{t("historyKeys.requested")}</p> : null}
    <button type="button" className="message-send-action" disabled={status?.kind === "sending"} onClick={onRequest}>
      <KeyRound size={13} aria-hidden="true" />
      <span>{t(status?.kind === "sending" ? "historyKeys.sending" : forceAll ? "historyKeys.requestAll" : requested ? "historyKeys.retry" : "historyKeys.request")}</span>
    </button>
    {status?.kind === "failed" || status?.kind === "partial" ? <p className="profile-settings-hint error">{t(failureMessages[status.reason])}</p> : null}
    <p className="profile-settings-hint">{t("historyKeys.explanation")}</p>
  </div>;
}
