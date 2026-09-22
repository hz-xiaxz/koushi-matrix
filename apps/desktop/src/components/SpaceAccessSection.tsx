import { type Ref, useEffect, useRef, useState } from "react";

import { t, type MessageId } from "../i18n/messages";
import type { RoomJoinRule, RoomManagementState, SpaceSummary } from "../domain/types";

/** The two choices this section offers. Private is the `invite` rule, never `private`. */
type AccessChoice = "public" | "invite";

/**
 * Issue #935: who can join a Space, shown to every member and changeable by a
 * member Rust says may send `m.room.join_rules`.
 *
 * The rule shown is Rust's: the loaded settings snapshot when it is this
 * Space's (kept current by the reducer when another client changes the rule),
 * else the join rule synced with the room list. Only the join rule changes —
 * child rooms, history, encryption and directory publication are untouched,
 * as in Element's Space visibility settings.
 */
export function SpaceAccessSection({
  headingRef,
  roomManagement,
  space,
  onUpdateJoinRule
}: {
  headingRef?: Ref<HTMLHeadingElement>;
  roomManagement?: RoomManagementState;
  space: SpaceSummary;
  onUpdateJoinRule?: (spaceId: string, joinRule: RoomJoinRule) => void | Promise<void>;
}) {
  const [confirming, setConfirming] = useState<AccessChoice | null>(null);
  const [submission, setSubmission] = useState<{
    target: AccessChoice;
    /** A failure already on screen when this submission started is not its outcome. */
    priorFailureRequestId: number | null;
    /** Until the command settles, so a second click cannot race Rust's Pending. */
    inFlight: boolean;
    rejected: boolean;
  } | null>(null);
  const epochRef = useRef(0);
  useEffect(
    () => () => {
      epochRef.current += 1;
    },
    []
  );

  const spaceId = space.space_id;
  const settings =
    roomManagement?.selected_room_id === spaceId && roomManagement.settings?.room_id === spaceId
      ? roomManagement.settings
      : null;
  const current: RoomJoinRule | null = settings?.join_rule ?? space.join_rule ?? null;
  const operation = roomManagement?.operation ?? { kind: "idle" as const };
  const operationIsThisSpace =
    operation.kind !== "idle" && operation.room_id === spaceId && operation.operation === "settings";
  const pending =
    (operationIsThisSpace && operation.kind === "pending") || Boolean(submission?.inFlight);
  const failure =
    operationIsThisSpace &&
    operation.kind === "failed" &&
    submission !== null &&
    operation.request_id !== submission.priorFailureRequestId
      ? operation.failureKind
      : null;
  const failed = !pending && (failure !== null || Boolean(submission?.rejected));
  const saved = submission !== null && !pending && !failed && current === submission.target;
  const canChange = Boolean(settings?.permissions.can_change_join_rule && onUpdateJoinRule);
  const choices = (["public", "invite"] as const).filter((choice) => choice !== current);

  function submit(target: AccessChoice) {
    const epoch = ++epochRef.current;
    setConfirming(null);
    setSubmission({
      target,
      priorFailureRequestId: operation.kind === "failed" ? operation.request_id : null,
      inFlight: true,
      rejected: false
    });
    const settle = (rejected: boolean) => {
      if (epochRef.current === epoch) {
        setSubmission((previous) =>
          previous ? { ...previous, inFlight: false, rejected } : previous
        );
      }
    };
    try {
      void Promise.resolve(onUpdateJoinRule?.(spaceId, target)).then(
        () => settle(false),
        () => settle(true)
      );
    } catch {
      settle(true);
    }
  }

  return (
    <section className="settings-section space-access" aria-labelledby="space-access-title">
      <h3 id="space-access-title" ref={headingRef} tabIndex={-1}>
        {t("space.access")}
      </h3>
      <p className="profile-settings-hint">{t("space.accessScope")}</p>
      <div className="settings-detail-list">
        <div className="settings-detail-row">
          <span>{t("space.accessCurrent")}</span>
          <strong className="space-access-mode">
            {current ? t(accessModeMessage(current)) : t("space.accessLoading")}
          </strong>
        </div>
      </div>

      {current === null ? null : !settings ? (
        <p className="profile-settings-hint">{t("space.accessCheckingPermission")}</p>
      ) : !canChange ? (
        <p className="profile-settings-hint">{t("space.accessNoPermission")}</p>
      ) : (
        <div className="space-access-actions">
          {confirming ? (
            <div
              className="space-access-confirm"
              role="group"
              aria-label={t(choiceLabel(confirming))}
            >
              <p>
                {t(confirming === "public" ? "space.accessConfirmPublic" : "space.accessConfirmPrivate")}
              </p>
              {current !== "public" && current !== "invite" ? (
                <p>{t("space.accessReplacesRule", { rule: joinRuleName(current) })}</p>
              ) : null}
              <div className="profile-settings-actions">
                <button
                  className="profile-settings-action"
                  type="button"
                  disabled={pending}
                  onClick={() => submit(confirming)}
                >
                  {t(choiceLabel(confirming))}
                </button>
                <button
                  className="profile-settings-action"
                  type="button"
                  onClick={() => setConfirming(null)}
                >
                  {t("action.cancel")}
                </button>
              </div>
            </div>
          ) : (
            <div className="profile-settings-actions">
              {choices.map((choice) => (
                <button
                  className="profile-settings-action"
                  key={choice}
                  type="button"
                  disabled={pending}
                  onClick={() => setConfirming(choice)}
                >
                  {t(choiceLabel(choice))}
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {pending || failed || saved ? (
        <p
          className={failed ? "space-access-status space-access-status-failed" : "space-access-status"}
          role="status"
        >
          {pending
            ? t("space.accessSaving")
            : failed
              ? t(failure === "forbidden" ? "space.accessForbidden" : "space.accessFailed")
              : t("space.accessSaved")}
        </p>
      ) : null}
    </section>
  );
}

function choiceLabel(choice: AccessChoice): MessageId {
  return choice === "public" ? "space.accessMakePublic" : "space.accessMakePrivate";
}

function accessModeMessage(rule: RoomJoinRule): MessageId {
  switch (rule) {
    case "public":
      return "space.accessPublic";
    case "invite":
      return "space.accessInvite";
    case "knock":
      return "space.accessKnock";
    case "restricted":
      return "space.accessRestricted";
    case "knockRestricted":
      return "space.accessKnockRestricted";
    case "private":
      return "space.accessPrivate";
    case "unknown":
      return "space.accessUnknown";
  }
}

function joinRuleName(rule: RoomJoinRule): string {
  switch (rule) {
    case "public":
      return t("room.joinRulePublic");
    case "invite":
      return t("room.joinRuleInvite");
    case "knock":
      return t("room.joinRuleKnock");
    case "restricted":
      return t("room.joinRuleRestricted");
    case "knockRestricted":
      return t("room.joinRuleKnockRestricted");
    case "private":
      return t("room.joinRulePrivate");
    case "unknown":
      return t("room.joinRuleUnknown");
  }
}
