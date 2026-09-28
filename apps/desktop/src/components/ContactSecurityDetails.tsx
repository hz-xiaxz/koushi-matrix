import { AlertTriangle, Info } from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { t, type MessageId } from "../i18n/messages";
import type {
  ContactDeviceSignature,
  ContactIdentityVerification,
  ContactSecurityState,
  ContactSecuritySummary
} from "../domain/types";
import { ICON_SIZE } from "../app/uiShared";

/** Typed command dispatchers; the Rust snapshot is the only displayed state. */
export interface ContactSecurityActions {
  load: (userId: string) => void;
  close: () => void;
}

type DevicesView =
  | "checking"
  | "unavailable"
  | "allOwnerSigned"
  | "someNotOwnerSigned"
  | "noDevices"
  | "ownerIdentityMissing";

type IdentityView = ContactIdentityVerification | "checking" | "unavailable";

const DEVICE_STATUS: Record<DevicesView, MessageId> = {
  checking: "people.security.devicesChecking",
  unavailable: "people.security.devicesUnavailable",
  allOwnerSigned: "people.security.devicesAllConfirmed",
  someNotOwnerSigned: "people.security.devicesSomeUnconfirmed",
  noDevices: "people.security.devicesNone",
  ownerIdentityMissing: "people.security.devicesNoIdentity"
};

const DEVICE_EXPLANATION: Partial<Record<DevicesView, MessageId>> = {
  unavailable: "people.security.devicesUnavailableExplain",
  allOwnerSigned: "people.security.devicesAllConfirmedExplain",
  someNotOwnerSigned: "people.security.devicesSomeUnconfirmedExplain",
  noDevices: "people.security.devicesNoneExplain",
  ownerIdentityMissing: "people.security.devicesNoIdentityExplain"
};

const IDENTITY_STATUS: Record<IdentityView, MessageId> = {
  checking: "people.security.devicesChecking",
  unavailable: "people.security.devicesUnavailable",
  verifiedByYou: "people.security.identityVerified",
  notVerifiedByYou: "people.security.identityNotVerified",
  changedAfterVerification: "people.security.identityChanged",
  unknown: "people.security.identityUnknown"
};

const IDENTITY_EXPLANATION: Partial<Record<IdentityView, MessageId>> = {
  verifiedByYou: "people.security.identityVerifiedExplain",
  notVerifiedByYou: "people.security.identityNotVerifiedExplain",
  changedAfterVerification: "people.security.identityChangedExplain",
  unknown: "people.security.identityUnknownExplain"
};

const DEVICE_SIGNATURE: Record<ContactDeviceSignature, MessageId> = {
  ownerSigned: "people.security.deviceOwnerSigned",
  notOwnerSigned: "people.security.deviceNotOwnerSigned",
  ownerSignatureInvalid: "people.security.deviceSignatureInvalid",
  ownerIdentityMissing: "people.security.deviceNoIdentity"
};

/**
 * Security details in User info (#1024). Opening dispatches only the
 * read-only load; expanding an explanation never changes trust or sending
 * policy. Routine states (unconfirmed devices, a contact you never verified)
 * are neutral; only an identity change after your verification is an
 * attention state.
 */
export function ContactSecurityDetails({
  userId,
  state,
  actions
}: {
  userId: string;
  state: ContactSecurityState;
  actions: ContactSecurityActions;
}) {
  const actionsRef = useRef(actions);
  actionsRef.current = actions;

  useEffect(() => {
    actionsRef.current.load(userId);
    return () => actionsRef.current.close();
  }, [userId]);

  // The Rust state is fenced by user id; anything else is not this contact.
  const current = state.user_id === userId ? state : null;
  const summary = current?.summary ?? null;
  const unavailable = current?.load.kind === "failed";
  const devicesView: DevicesView = summary
    ? summary.devices
    : unavailable
      ? "unavailable"
      : "checking";
  const identityView: IdentityView = summary
    ? summary.identity
    : unavailable
      ? "unavailable"
      : "checking";

  return (
    <section className="profile-security" aria-labelledby="profile-security-title">
      <h3 id="profile-security-title">{t("people.security.title")}</h3>
      <SecurityRow
        key={`devices:${userId}`}
        label={t("people.security.devicesLabel")}
        status={t(DEVICE_STATUS[devicesView])}
        statusKind={devicesView}
        attention={false}
        action={
          unavailable ? (
            <button
              className="profile-text-button"
              type="button"
              onClick={() => actionsRef.current.load(userId)}
            >
              {t("people.security.retry")}
            </button>
          ) : null
        }
      >
        {devicesView === "checking" ? null : (
          <DevicesExplanation view={devicesView} summary={summary} />
        )}
      </SecurityRow>
      <SecurityRow
        key={`identity:${userId}`}
        label={t("people.security.identityLabel")}
        status={t(IDENTITY_STATUS[identityView])}
        statusKind={identityView}
        attention={identityView === "changedAfterVerification"}
      >
        {IDENTITY_EXPLANATION[identityView] ? (
          <p>{t(IDENTITY_EXPLANATION[identityView]!)}</p>
        ) : null}
      </SecurityRow>
      <p className="profile-security-scope">{t("people.security.scope")}</p>
    </section>
  );
}

function SecurityRow({
  label,
  status,
  statusKind,
  attention,
  action = null,
  children
}: {
  label: string;
  status: string;
  statusKind: string;
  attention: boolean;
  /** An action for this property, kept in the same row as its status. */
  action?: ReactNode;
  children: ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  const panelId = useId();
  const hasDetails = children !== null && children !== undefined && children !== false;
  const Icon = attention ? AlertTriangle : Info;
  return (
    <div
      className={`profile-security-row${attention ? " is-attention" : ""}`}
      data-status={statusKind}
    >
      <div className="profile-security-summary">
        <span className="profile-security-label">{label}</span>
        <span className="profile-security-status">
          <Icon size={ICON_SIZE.small} aria-hidden="true" />
          <strong>{status}</strong>
        </span>
        {action}
        {hasDetails ? (
          <button
            className="profile-text-button"
            type="button"
            aria-expanded={expanded}
            aria-controls={panelId}
            aria-label={t("people.security.detailsFor", { topic: label })}
            onClick={() => setExpanded((value) => !value)}
          >
            {t("people.security.details")}
          </button>
        ) : null}
      </div>
      {hasDetails && expanded ? (
        <div className="profile-security-explanation" id={panelId}>
          {children}
        </div>
      ) : null}
    </div>
  );
}

function DevicesExplanation({
  view,
  summary
}: {
  view: DevicesView;
  summary: ContactSecuritySummary | null;
}) {
  const explanation = DEVICE_EXPLANATION[view];
  const counts = summary?.device_counts;
  return (
    <>
      {explanation ? <p>{t(explanation)}</p> : null}
      {summary && counts && counts.total > 0 ? (
        <>
          {view !== "ownerIdentityMissing" ? (
            <p>
              {t("people.security.devicesCount", {
                confirmed: counts.owner_signed,
                total: counts.total
              })}
            </p>
          ) : null}
          {counts.owner_signature_invalid > 0 ? (
            <p>
              {t("people.security.devicesInvalidCount", {
                count: counts.owner_signature_invalid
              })}
            </p>
          ) : null}
          <ul className="profile-security-devices">
            {summary.device_signatures.map((signature, index) => (
              <li key={index} data-signature={signature}>
                <span>{t("people.security.deviceOrdinal", { index: index + 1 })}</span>
                <span>{t(DEVICE_SIGNATURE[signature])}</span>
              </li>
            ))}
          </ul>
          <p>{t("people.security.devicesHowItWorks")}</p>
        </>
      ) : null}
      {counts && counts.excluded_dehydrated > 0 ? (
        <p>
          {t("people.security.devicesDehydratedCount", { count: counts.excluded_dehydrated })}
        </p>
      ) : null}
    </>
  );
}
