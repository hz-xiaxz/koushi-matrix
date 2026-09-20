import {
  Bell,
  ChevronRight,
  FileText,
  Home,
  MailPlus,
  Settings,
  SlidersHorizontal,
  Users
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { t } from "../i18n/messages";
import type {
  RoomManagementState,
  RoomSummary,
  SpaceChildMembership,
  SpaceChildSummary,
  SpaceSummary
} from "../domain/types";
import { ImeTextField } from "./ImeTextControl";

export function SpaceInfoPanel({
  fallbackName,
  localIcon = "",
  localName = "",
  rooms,
  roomManagement,
  space,
  spaceChildren = [],
  onAcceptInvite,
  onInvitePeople,
  onJoinRoom,
  onOpenFiles,
  onOpenMembers,
  onSetLocalPresentation
}: {
  fallbackName: string;
  localIcon?: string;
  localName?: string;
  rooms: RoomSummary[];
  roomManagement?: RoomManagementState;
  space: SpaceSummary | null;
  /** Issue #961: every child the Space advertises, joined or not. */
  spaceChildren?: readonly SpaceChildSummary[];
  onAcceptInvite?: (roomId: string) => void;
  onInvitePeople?: () => void;
  onJoinRoom?: (roomId: string) => void;
  onOpenFiles?: () => void;
  onOpenMembers?: () => void;
  onSetLocalPresentation?: (override: { name?: string; icon?: string } | null) => void;
}) {
  const [localNameDraft, setLocalNameDraft] = useState(localName);
  const [localIconDraft, setLocalIconDraft] = useState(localIcon);
  const childRooms = space
    ? space.child_room_ids
        .map((roomId) => rooms.find((room) => room.room_id === roomId))
        .filter((room): room is RoomSummary => Boolean(room && !room.is_dm))
    : rooms.filter((room) => !room.is_dm);
  const unreadTotal = childRooms.reduce((sum, room) => sum + room.unread_count, 0);
  // Joined children are already listed above from the room list, which owns
  // their labels and unread state; this is the remainder of the Space.
  const joinedRoomIds = new Set(childRooms.map((room) => room.room_id));
  const outsideChildren = spaceChildren.filter(
    (child) => !joinedRoomIds.has(child.room_id) && child.membership !== "joined"
  );
  const title = localName.trim() || space?.display_name || fallbackName;
  const loadedSpaceSettings =
    space && roomManagement?.selected_room_id === space.space_id
      ? roomManagement.settings
      : null;
  const memberCount = loadedSpaceSettings?.members.length ?? 0;

  useEffect(() => {
    setLocalNameDraft(localName);
    setLocalIconDraft(localIcon);
  }, [localIcon, localName]);

  function openMembers() {
    onOpenMembers?.();
  }

  function updateLocalPresentation(next: { name: string; icon: string }) {
    setLocalNameDraft(next.name);
    setLocalIconDraft(next.icon);
    onSetLocalPresentation?.(next);
  }

  return (
    <section className="settings-panel space-info-panel" aria-labelledby="space-info-title">
      <header className="settings-panel-header">
        <div>
          <h2 id="space-info-title" dir="auto">{title}</h2>
          <p dir="auto">{space?.space_id ?? t("space.allRooms")}</p>
        </div>
      </header>

      <div className="settings-summary-grid" aria-label={t("space.summary")}>
        <SummaryTile label={t("workspace.rooms")} value={String(childRooms.length)} />
        <SummaryTile label={t("room.members")} value={loadedSpaceSettings ? String(memberCount) : "-"} />
        <SummaryTile label={t("room.unread")} value={String(unreadTotal)} />
      </div>

      {space ? (
        <section className="settings-section" aria-label={t("space.names")}>
          <h3>{t("space.names")}</h3>
          <div className="settings-detail-list">
            {/*
              Issue #960: the canonical `m.room.name` and the local label this
              device shows are different facts. A Space with no name event has
              no canonical name — its alias or computed name is not one.
            */}
            <DetailRow
              label={t("space.canonicalName")}
              userText={Boolean(space.raw_name?.trim())}
              value={space.raw_name?.trim() || t("space.nameUnset")}
            />
            <DetailRow
              label={t("space.localName")}
              userText={Boolean(localName.trim())}
              value={localName.trim() || t("space.nameUnset")}
            />
          </div>
        </section>
      ) : null}

      {space && onSetLocalPresentation ? (
        <section className="settings-section" aria-label={t("space.localPresentation")}>
          <h3>{t("space.localPresentation")}</h3>
          <div className="profile-settings-form">
            <label className="profile-settings-field">
              <span>{t("space.localName")}</span>
              <ImeTextField
                value={localNameDraft}
                syncKey={`${space.space_id}:local-name`}
                placeholder={t("space.localNamePlaceholder")}
                onChange={(event) =>
                  updateLocalPresentation({
                    name: event.currentTarget.value,
                    icon: localIconDraft
                  })
                }
              />
            </label>
            <label className="profile-settings-field">
              <span>{t("space.localIcon")}</span>
              <ImeTextField
                value={localIconDraft}
                syncKey={`${space.space_id}:local-icon`}
                placeholder={t("space.localIconPlaceholder")}
                maxLength={12}
                onChange={(event) =>
                  updateLocalPresentation({
                    name: localNameDraft,
                    icon: event.currentTarget.value
                  })
                }
              />
            </label>
            <div className="profile-settings-actions">
              <button
                className="profile-settings-action"
                type="button"
                onClick={() => {
                  setLocalNameDraft("");
                  setLocalIconDraft("");
                  onSetLocalPresentation(null);
                }}
              >
                {t("space.resetLocalPresentation")}
              </button>
            </div>
          </div>
        </section>
      ) : null}

      <section className="settings-section" aria-label={t("workspace.rooms")}>
        <h3>{t("workspace.rooms")}</h3>
        <div className="settings-detail-list">
          {childRooms.map((room) => (
            <div className="settings-detail-row" key={room.room_id}>
              <span dir="auto">{room.display_label}</span>
              <small dir="auto">{room.unread_count ? t("room.unreadCount", { count: room.unread_count }) : room.room_id}</small>
            </div>
          ))}
          {/*
            Issue #961: the rest of the Space — children the account has not
            joined — with the relationship it is in, and a join action only
            where the server's own join rule allows one.
          */}
          {outsideChildren.map((child) => (
            <div className="settings-detail-row" key={child.room_id}>
              <span dir="auto">{child.display_name}</span>
              <small className="space-child-status">
                <span className="room-membership-badge">
                  {spaceChildMembershipLabel(child.membership)}
                </span>
                {/*
                  An invitation is answered through the invite workflow, which
                  owns the account's invite list; only a room with no
                  invitation is entered with a join.
                */}
                {child.membership === "invited" && onAcceptInvite ? (
                  <button
                    className="profile-settings-action"
                    type="button"
                    onClick={() => onAcceptInvite(child.room_id)}
                  >
                    {t("invite.accept")}
                  </button>
                ) : child.can_join && onJoinRoom ? (
                  <button
                    className="profile-settings-action"
                    type="button"
                    onClick={() => onJoinRoom(child.room_id)}
                  >
                    {t("directory.join")}
                  </button>
                ) : null}
              </small>
            </div>
          ))}
        </div>
      </section>

      <section className="settings-section" aria-label={t("space.spacePreferences")}>
        <h3>{t("space.spacePreferences")}</h3>
        <div className="settings-detail-list">
          <DetailRow label={t("space.roomMembership")} value={space ? t("space.childRooms") : t("space.allRooms")} />
          <DetailRow label={t("space.directMessages")} value={t("room.globalDmList")} />
          <DetailRow label={t("room.notifications")} value={unreadTotal ? t("room.unreadCount", { count: unreadTotal }) : t("space.noUnread")} />
        </div>
      </section>

      <SettingsEntryList
        entries={[
          { icon: <Home size={16} />, label: t("space.home") },
          { icon: <SlidersHorizontal size={16} />, label: t("space.preferences") },
          { icon: <Settings size={16} />, label: t("space.spaceSettings") },
          { icon: <Users size={16} />, label: t("room.members"), onClick: space ? openMembers : undefined },
          { icon: <MailPlus size={16} />, label: t("space.invite"), onClick: onInvitePeople },
          { icon: <Bell size={16} />, label: t("room.notifications") },
          { icon: <FileText size={16} />, label: t("room.files"), onClick: onOpenFiles }
        ]}
      />
    </section>
  );
}

function spaceChildMembershipLabel(membership: SpaceChildMembership): string {
  switch (membership) {
    case "invited":
      return t("roomList.membershipInvited");
    case "knocked":
      return t("roomList.membershipKnocked");
    case "unknown":
      return t("roomList.membershipUnknown");
    default:
      return t("roomList.membershipNotJoined");
  }
}

function DetailRow({
  label,
  value,
  userText = false
}: {
  label: string;
  value: string;
  /** Set for values that carry user-provided text, which needs `dir="auto"`. */
  userText?: boolean;
}) {
  return (
    <div className="settings-detail-row">
      <span>{label}</span>
      <small dir={userText ? "auto" : undefined}>{value}</small>
    </div>
  );
}

function SummaryTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="settings-summary-tile">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function SettingsEntryList({
  entries
}: {
  entries: Array<{ icon: ReactNode; label: string; onClick?: () => void }>;
}) {
  return (
    <div className="settings-list">
      {entries.map((entry) => (
        <button
          className="settings-list-item"
          key={entry.label}
          type="button"
          disabled={!entry.onClick}
          onClick={entry.onClick}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              {entry.icon}
            </span>
            <span>{entry.label}</span>
          </span>
          <ChevronRight size={14} />
        </button>
      ))}
    </div>
  );
}
