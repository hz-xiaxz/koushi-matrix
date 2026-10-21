import { useEffect, useRef, useState } from "react";
import { operationFailureLabel } from "../app/uiShared";
import type { RoomManagementState } from "../domain/types";
import { t } from "../i18n/messages";
import { InlineTextPropertyEditor, type PropertySaveStatus } from "./SettingsPropertyCard";

/** Confirmed shared topic and permission come exclusively from Rust settings. */
export function SpaceDescriptionSection({ spaceId, roomManagement, onUpdateTopic }: {
  spaceId: string;
  roomManagement?: RoomManagementState;
  onUpdateTopic?: (spaceId: string, topic: string | null) => void | Promise<void>;
}) {
  const [submission, setSubmission] = useState<{
    target: string | null;
    inFlight: boolean;
    rejected: boolean;
    terminal: PropertySaveStatus;
  } | null>(null);
  const epochRef = useRef(0);
  useEffect(() => () => { epochRef.current += 1; }, []);
  const settings = roomManagement?.selected_room_id === spaceId && roomManagement.settings?.room_id === spaceId
    ? roomManagement.settings : null;
  const operation = roomManagement?.operation ?? { kind: "idle" as const };
  const ownOperation = operation.kind !== "idle" && operation.room_id === spaceId && operation.operation === "settings";
  const pending = (ownOperation && operation.kind === "pending") || Boolean(submission?.inFlight);
  const failureMessage = !pending && submission?.rejected ? operationFailureLabel("sdk") : null;
  const saved = Boolean(settings && submission && !pending && !failureMessage && settings.topic === submission.target);
  // Attribute a Rust outcome once; later operations on other properties cannot
  // relabel this card's completed submission.
  useEffect(() => {
    if (submission && !submission.terminal && (failureMessage || saved)) {
      setSubmission({ ...submission, terminal: failureMessage ? { kind: "failed", message: failureMessage } : { kind: "saved" } });
    }
  }, [submission, failureMessage, saved]);
  const status: PropertySaveStatus = !settings || !submission ? null : submission.terminal ??
    (pending ? { kind: "saving" } : failureMessage ? { kind: "failed", message: failureMessage } : saved ? { kind: "saved" } : null);
  const canEdit = Boolean(settings?.permissions.can_change_topic && onUpdateTopic);
  function submit(target: string | null) {
    if (!canEdit || pending) return;
    const epoch = ++epochRef.current;
    setSubmission({ target, inFlight: true, rejected: false, terminal: null });
    const settle = (rejected: boolean) => {
      if (epoch === epochRef.current) setSubmission(previous => previous ? { ...previous, inFlight: false, rejected } : previous);
    };
    try { void Promise.resolve(onUpdateTopic?.(spaceId, target)).then(() => settle(false), () => settle(true)); }
    catch { settle(true); }
  }
  return <section className="settings-section">
    <InlineTextPropertyEditor
      property="space-description" label={t("space.description")} inputLabel={t("space.description")}
      value={settings?.topic ?? ""} editValue={failureMessage || submission?.terminal?.kind === "failed" ? submission?.target ?? "" : undefined} emptyText={t(settings ? "space.noDescription" : "space.descriptionLoading")}
      display={settings?.topic ? <span className="settings-property-multiline">{settings.topic}</span> : undefined}
      editLabel={t("space.editDescription")} saveLabel={t("space.saveDescription")} clearLabel={t("space.clearDescription")}
      multiline syncKey={`space-description:${spaceId}`} canEdit={canEdit} busy={pending}
      readOnlyReason={settings && !settings.permissions.can_change_topic ? t("space.descriptionNoPermission") : null}
      hint={<p className="profile-settings-hint">{t("space.descriptionShared")}</p>}
      status={status} onSave={value => submit(value || null)} onClear={() => submit(null)}
    />
  </section>;
}
