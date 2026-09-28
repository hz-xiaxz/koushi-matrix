import { AlertTriangle, Info } from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { t, type MessageId } from "../i18n/messages";
import type {
  ContactDeviceSignature,
  ContactIdentityVerification,
  ContactSecurityState,
  ContactSecuritySummary,
  ContactVerificationDirectChat,
  ContactVerificationOffer,
  TrustOperationFailureKind,
  VerificationFlowState
} from "../domain/types";
import { ICON_SIZE } from "../app/uiShared";

/** Typed command dispatchers; the Rust snapshot is the only displayed state. */
export interface ContactSecurityActions {
  load: (userId: string) => void;
  close: () => void;
  /** Verify user: send the SAS request (only after the confirmation step). */
  requestVerification: (userId: string) => void;
  acceptVerification: (flowId: number) => void;
  confirmSas: (flowId: number) => void;
  mismatchSas: (flowId: number) => void;
  cancelVerification: (flowId: number) => void;
}

const DIRECT_CHAT_EXPLANATION: Record<ContactVerificationDirectChat, MessageId> = {
  existingEncrypted: "people.security.verifyChatExistingEncrypted",
  existingUnencrypted: "people.security.verifyChatExistingUnencrypted",
  new: "people.security.verifyChatNew"
};

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
  verification,
  actions
}: {
  userId: string;
  state: ContactSecurityState;
  /** The shared Rust verification flow; shown here only for this contact. */
  verification: VerificationFlowState;
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
      <VerifyUserPanel
        key={`verify:${userId}`}
        userId={userId}
        offer={summary?.verification ?? null}
        identity={summary?.identity ?? null}
        busy={current?.verification_busy ?? false}
        verification={verification}
        actionsRef={actionsRef}
      />
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

function flowForContact(
  verification: VerificationFlowState,
  userId: string
): Exclude<VerificationFlowState, { kind: "idle" }> | null {
  if (verification.kind === "idle") return null;
  return verification.target.user_id === userId ? verification : null;
}

function failureMessage(kind: TrustOperationFailureKind): MessageId {
  switch (kind) {
    case "cancelled":
      return "people.security.verifyFailedCancelled";
    case "mismatch":
      return "people.security.verifyFailedMismatch";
    case "timeout":
      return "people.security.verifyFailedTimeout";
    default:
      return "people.security.verifyFailedOther";
  }
}

/**
 * Verify user (#1024), kept with "Your verification". Rust decides whether
 * it is offered; React owns only the confirmation step's visibility. The
 * request is dispatched only from the confirmation step's Send button.
 */
function VerifyUserPanel({
  userId,
  offer,
  identity,
  busy,
  verification,
  actionsRef
}: {
  userId: string;
  offer: ContactVerificationOffer | null;
  identity: ContactIdentityVerification | null;
  /** Rust: another verification flow is in progress. */
  busy: boolean;
  verification: VerificationFlowState;
  actionsRef: { current: ContactSecurityActions };
}) {
  const [confirming, setConfirming] = useState(false);
  const titleId = useId();
  const flow = flowForContact(verification, userId);
  const active =
    flow !== null &&
    (flow.kind === "requested" ||
      flow.kind === "accepted" ||
      flow.kind === "sasPresented" ||
      flow.kind === "confirming");

  useEffect(() => {
    if (active || busy) setConfirming(false);
  }, [active, busy]);

  if (active && flow) {
    const flowId = flow.request_id;
    return (
      <div className="profile-security-verify" role="group" aria-labelledby={titleId}>
        <strong id={titleId}>{t("people.security.verifyConfirmTitle")}</strong>
        {flow.kind === "requested" && flow.initiator === "us" ? (
          <p role="status">{t("people.security.verifyWaiting")}</p>
        ) : null}
        {flow.kind === "requested" && flow.initiator === "them" ? (
          <p role="status">{t("people.security.verifyIncoming")}</p>
        ) : null}
        {flow.kind === "accepted" ? (
          <p role="status">{t("people.security.verifyStarting")}</p>
        ) : null}
        {flow.kind === "sasPresented" || flow.kind === "confirming" ? (
          <>
            <p>{t("people.security.verifyCompare")}</p>
            <ol className="trust-sas-list" aria-label={t("people.security.verifyEmojiList")}>
              {flow.emojis.map((emoji, index) => (
                <li className="trust-sas-item" key={`${emoji.symbol}-${index}`}>
                  {emoji.symbol}
                </li>
              ))}
            </ol>
          </>
        ) : null}
        {flow.kind === "confirming" ? (
          <p role="status">{t("people.security.verifyConfirming")}</p>
        ) : null}
        <div className="profile-security-verify-actions">
          {flow.kind === "requested" && flow.initiator === "them" ? (
            <button
              className="dialog-button is-primary"
              type="button"
              onClick={() => actionsRef.current.acceptVerification(flowId)}
            >
              {t("people.security.verifyAccept")}
            </button>
          ) : null}
          {flow.kind === "sasPresented" ? (
            <>
              <button
                className="dialog-button is-primary"
                type="button"
                onClick={() => actionsRef.current.confirmSas(flowId)}
              >
                {t("people.security.verifyMatch")}
              </button>
              <button
                className="dialog-button"
                type="button"
                onClick={() => actionsRef.current.mismatchSas(flowId)}
              >
                {t("people.security.verifyNoMatch")}
              </button>
            </>
          ) : null}
          <button
            className="dialog-button"
            type="button"
            onClick={() => actionsRef.current.cancelVerification(flowId)}
          >
            {t("action.cancel")}
          </button>
        </div>
      </div>
    );
  }

  const settled =
    flow?.kind === "done" ? (
      <p role="status">{t("people.security.verifyDone")}</p>
    ) : flow?.kind === "failed" ? (
      <p role="status">{t(failureMessage(flow.failureKind))}</p>
    ) : null;

  if (!offer || offer.kind === "notOffered") {
    return settled ? <div className="profile-security-verify">{settled}</div> : null;
  }
  if (offer.kind === "requiresYourCrossSigning") {
    return (
      <div className="profile-security-verify">
        {settled}
        <p>{t("people.security.verifyRequiresCrossSigning")}</p>
      </div>
    );
  }
  if (busy) {
    return (
      <div className="profile-security-verify">
        {settled}
        <p>{t("people.security.verifyBusy")}</p>
      </div>
    );
  }
  const label =
    identity === "changedAfterVerification"
      ? t("people.security.verifyAgain")
      : t("people.security.verifyUser");
  if (!confirming) {
    return (
      <div className="profile-security-verify">
        {settled}
        <button className="profile-text-button" type="button" onClick={() => setConfirming(true)}>
          {label}
        </button>
      </div>
    );
  }
  return (
    <div className="profile-security-verify" role="group" aria-labelledby={titleId}>
      {settled}
      <strong id={titleId}>{t("people.security.verifyConfirmTitle")}</strong>
      <p>{t(DIRECT_CHAT_EXPLANATION[offer.direct_chat])}</p>
      <p>{t("people.security.verifyHow")}</p>
      <div className="profile-security-verify-actions">
        <button
          className="dialog-button is-primary"
          type="button"
          onClick={() => {
            setConfirming(false);
            actionsRef.current.requestVerification(userId);
          }}
        >
          {t("people.security.verifySend")}
        </button>
        <button
          className="dialog-button"
          type="button"
          onClick={() => setConfirming(false)}
        >
          {t("action.cancel")}
        </button>
      </div>
    </div>
  );
}
