import { X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import type { SpaceMemberEntry, SpaceMembersState, UserProfile } from "../domain/types";
import { contextMenuItems } from "../domain/contextMenus";
import { t } from "../i18n/messages";
import { ICON_SIZE, type OpenContextMenu } from "../app/uiShared";
import { ImeTextField } from "./ImeTextControl";
import { EntityAvatar } from "./Shell";

export type SpaceInviteAvailabilityReason =
  | "available"
  | "settings_unavailable"
  | "permission_denied"
  | "operation_pending"
  | "invite_pending";

export interface SpaceMembersPanelProps {
  state: SpaceMembersState;
  canInvite: boolean;
  onClose?: () => void;
  profileUsers?: Record<string, UserProfile>;
  onRequestAvatarThumbnail?: (mxcUri: string) => void | Promise<void>;
  childRoomLabels?: ReadonlyMap<string, string>;
  onInviteUser: (userId: string) => void;
  onOpenProfile: (userId: string) => void;
  onOpenContextMenu?: OpenContextMenu;
  onDiagnostic?: (message: string) => void;
  inviteAvailabilityReason?: SpaceInviteAvailabilityReason;
}

interface SpaceMembersSection {
  id: "joined" | "invited" | "child-only";
  label: string;
  entries: SpaceMemberEntry[];
}

function memberInitials(entry: SpaceMemberEntry): string {
  const words = entry.display_label.trim().split(/\s+/).filter(Boolean);
  const initials = words.length > 1 ? `${words[0]?.[0] ?? ""}${words[1]?.[0] ?? ""}` : words[0]?.slice(0, 2);
  return (initials || "?").toUpperCase();
}

function memberRoleLabel(role: SpaceMemberEntry["role"]): string | null {
  switch (role) {
    case "creator":
      return t("room.roleCreator");
    case "administrator":
      return t("room.roleAdministrator");
    default:
      return null;
  }
}

function matchesSearch(entry: SpaceMemberEntry, query: string): boolean {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) {
    return true;
  }

  return [entry.display_label, entry.display_name, entry.original_display_label, entry.user_id]
    .filter((value): value is string => Boolean(value))
    .some((value) => value.toLocaleLowerCase().includes(normalizedQuery));
}

function hasPendingOperation(state: SpaceMembersState): boolean {
  return (
    state.operation.kind === "loading" ||
    state.operation.kind === "inviting" ||
    state.operation.kind === "cancellingInvite"
  );
}

function inviteIsDisabled(
  state: SpaceMembersState,
  entry: SpaceMemberEntry,
  canInvite: boolean
): boolean {
  const inFlightInviteTarget =
    state.operation.kind === "inviting" && state.operation.user_id === entry.user_id;
  return !canInvite || entry.invite_pending || inFlightInviteTarget || hasPendingOperation(state);
}

function inviteAvailabilityReasonForEntry(
  state: SpaceMembersState,
  entry: SpaceMemberEntry,
  canInvite: boolean,
  availabilityReason: SpaceInviteAvailabilityReason | undefined
): SpaceInviteAvailabilityReason {
  if (!canInvite) {
    return availabilityReason ?? "permission_denied";
  }
  if (entry.invite_pending) {
    return "invite_pending";
  }
  if (hasPendingOperation(state)) {
    return "operation_pending";
  }
  return availabilityReason ?? "available";
}

function childRoomContext(
  entry: SpaceMemberEntry,
  childRoomLabels: ReadonlyMap<string, string>
): string {
  const labels = entry.child_room_ids
    .map((roomId) => childRoomLabels.get(roomId)?.trim() ?? "")
    .filter(Boolean);
  if (entry.child_room_ids.length <= 2 && labels.length === entry.child_room_ids.length) {
    return t("spaceMembers.childRoomContext", { rooms: labels.join(", ") });
  }
  return t("spaceMembers.childRoomCount", { count: entry.child_room_ids.length });
}

export function SpaceMembersPanel({
  state,
  canInvite,
  onClose = () => undefined,
  profileUsers = {},
  onRequestAvatarThumbnail,
  childRoomLabels = new Map<string, string>(),
  onInviteUser,
  onOpenProfile,
  onOpenContextMenu,
  onDiagnostic,
  inviteAvailabilityReason
}: SpaceMembersPanelProps) {
  const [query, setQuery] = useState("");
  const panelRef = useRef<HTMLElement | null>(null);
  const sections = useMemo<SpaceMembersSection[]>(
    () => [
      {
        id: "joined",
        label: t("spaceMembers.sectionJoined"),
        entries: state.space_joined
      },
      {
        id: "invited",
        label: t("spaceMembers.sectionInvited"),
        entries: state.space_invited
      },
      {
        id: "child-only",
        label: t("spaceMembers.sectionChildOnly"),
        entries: state.child_room_only
      }
    ],
    [state.child_room_only, state.space_invited, state.space_joined]
  );
  const filteredSections = sections.map((section) => ({
    ...section,
    entries: section.entries.filter((entry) => matchesSearch(entry, query))
  }));
  const resultCount = filteredSections.reduce((count, section) => count + section.entries.length, 0);
  const hasResults = filteredSections.some((section) => section.entries.length > 0);
  const panelAvailabilityReason: SpaceInviteAvailabilityReason = !canInvite
    ? inviteAvailabilityReason ?? "permission_denied"
    : hasPendingOperation(state)
      ? "operation_pending"
      : inviteAvailabilityReason ?? "available";

  useEffect(() => {
    onDiagnostic?.(
      [
        `rendered joined=${state.space_joined.length}`,
        `invited=${state.space_invited.length}`,
        `child_only=${state.child_room_only.length}`,
        `search_active=${Boolean(query.trim())}`,
        `result_count=${resultCount}`,
        `availability_reason=${panelAvailabilityReason}`,
        `incomplete_notice=${state.incomplete_child_room_count > 0}`
      ].join(" ")
    );
  }, [
    onDiagnostic,
    panelAvailabilityReason,
    query,
    resultCount,
    state.child_room_only.length,
    state.incomplete_child_room_count,
    state.space_invited.length,
    state.space_joined.length
  ]);

  return (
    <section
      className="space-members-panel"
      ref={panelRef}
      aria-labelledby="space-members-title"
    >
      <header className="space-members-header">
        <h2 id="space-members-title">{t("spaceMembers.title")}</h2>
        <span className="space-members-count" aria-label={t("spaceMembers.joinedCount", {
          count: state.space_joined.length
        })}>
          {state.space_joined.length}
        </span>
        <button
          className="icon-button space-members-close"
          type="button"
          aria-label={t("action.close", { title: t("spaceMembers.title") })}
          onClick={onClose}
        >
          <X size={ICON_SIZE.control} />
        </button>
      </header>

      <div className="space-members-search" role="search">
        <label className="visually-hidden" htmlFor="space-members-search-input">
          {t("spaceMembers.search")}
        </label>
        <ImeTextField
          id="space-members-search-input"
          type="search"
          aria-label={t("spaceMembers.search")}
          placeholder={t("spaceMembers.search")}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>

      {state.operation.kind === "failed" ? (
        <p className="space-members-invite-failure" role="alert">
          {state.operation.user_id !== null
            ? t("spaceMembers.inviteFailed")
            : t("spaceMembers.loadFailed")}
        </p>
      ) : null}

      {state.incomplete_child_room_count > 0 ? (
        <p className="space-members-sync-notice" role="status">
          {t("spaceMembers.syncIncomplete")}
        </p>
      ) : null}

      <div className="space-members-sections">
        {filteredSections.map((section) => (
          <section
            className="space-members-section"
            data-space-members-section={section.id}
            key={section.id}
            aria-labelledby={`space-members-section-${section.id}`}
          >
            <h3 id={`space-members-section-${section.id}`}>{section.label}</h3>
            {section.entries.length > 0 ? (
              <ul className="space-members-list" aria-label={section.label}>
                {section.entries.map((entry) => (
                  <SpaceMemberRow
                    key={entry.user_id}
                    entry={entry}
                    sectionId={section.id}
                    state={state}
                    canInvite={canInvite}
                    childRoomLabels={childRoomLabels}
                    profileUsers={profileUsers}
                    avatarViewportRef={panelRef}
                    onInviteUser={onInviteUser}
                    onOpenProfile={onOpenProfile}
                    onOpenContextMenu={onOpenContextMenu}
                    onDiagnostic={onDiagnostic}
                    inviteAvailabilityReason={inviteAvailabilityReason}
                    onRequestAvatarThumbnail={onRequestAvatarThumbnail}
                  />
                ))}
              </ul>
            ) : null}
          </section>
        ))}
      </div>

      {!hasResults ? (
        <p className="space-members-empty" role="status">
          {t("spaceMembers.noResults")}
        </p>
      ) : null}
    </section>
  );
}

interface SpaceMemberRowProps {
  entry: SpaceMemberEntry;
  sectionId: SpaceMembersSection["id"];
  state: SpaceMembersState;
  canInvite: boolean;
  childRoomLabels: ReadonlyMap<string, string>;
  profileUsers: Record<string, UserProfile>;
  avatarViewportRef: RefObject<Element | null>;
  onInviteUser: (userId: string) => void;
  onOpenProfile: (userId: string) => void;
  onOpenContextMenu?: OpenContextMenu;
  onDiagnostic?: (message: string) => void;
  inviteAvailabilityReason?: SpaceInviteAvailabilityReason;
  onRequestAvatarThumbnail?: (mxcUri: string) => void | Promise<void>;
}

function SpaceMemberRow({
  entry,
  sectionId,
  state,
  canInvite,
  childRoomLabels,
  profileUsers,
  avatarViewportRef,
  onInviteUser,
  onOpenProfile,
  onOpenContextMenu,
  onDiagnostic,
  inviteAvailabilityReason,
  onRequestAvatarThumbnail
}: SpaceMemberRowProps) {
  const rowRef = useRef<HTMLLIElement>(null);
  const requestedAvatarUriRef = useRef<string | null>(null);
  const avatar = profileUsers[entry.user_id]?.avatar ?? null;

  useEffect(() => {
    if (
      !onRequestAvatarThumbnail ||
      !avatar ||
      avatar.thumbnail.kind !== "notRequested" ||
      !rowRef.current ||
      requestedAvatarUriRef.current === avatar.mxc_uri ||
      typeof IntersectionObserver === "undefined"
    ) {
      return;
    }

    const row = rowRef.current;
    const mxcUri = avatar.mxc_uri;
    const avatarViewport = avatarViewportRef.current;
    const observer = new IntersectionObserver(
      (entries) => {
        if (
          requestedAvatarUriRef.current === mxcUri ||
          !entries.some((entry) => entry.isIntersecting)
        ) {
          return;
        }
        requestedAvatarUriRef.current = mxcUri;
        observer.disconnect();
        void onRequestAvatarThumbnail(mxcUri);
      },
      { root: avatarViewport }
    );
    observer.observe(row);

    return () => observer.disconnect();
  }, [avatar?.mxc_uri, avatar?.thumbnail.kind, avatarViewportRef, onRequestAvatarThumbnail]);

  const roleLabel = memberRoleLabel(entry.role);

  return (
    <li
      ref={rowRef}
      className="space-members-row"
      data-user-id={entry.user_id}
      onContextMenu={(event) => {
        if (sectionId !== "child-only" || !onOpenContextMenu || !state.selected_space_id) {
          return;
        }
        onOpenContextMenu(
          event,
          {
            kind: "spaceMember",
            spaceId: state.selected_space_id,
            userId: entry.user_id,
            generation: state.generation
          },
          contextMenuItems({
            kind: "spaceMember",
            spaceId: state.selected_space_id,
            userId: entry.user_id,
            generation: state.generation,
            canInvite,
            invitePending: entry.invite_pending,
            operationPending: hasPendingOperation(state)
          })
        );
      }}
    >
      <button
        className="space-members-row-main"
        type="button"
        aria-label={t("people.openProfile", { name: entry.display_label })}
        onClick={() => onOpenProfile(entry.user_id)}
      >
        <span
          className="space-members-avatar"
          role={avatar?.thumbnail.kind === "ready" ? "img" : undefined}
          aria-label={avatar?.thumbnail.kind === "ready" ? "" : undefined}
        >
          <EntityAvatar
            avatar={avatar}
            className="space-members-avatar-content"
            colorSeed={entry.user_id}
            fallback={memberInitials(entry)}
          />
        </span>
        <span className="space-members-row-text">
          <span className="space-members-name" dir="auto">
            {entry.display_label}
            {roleLabel ? <span className="space-members-role">{roleLabel}</span> : null}
          </span>
          {sectionId === "child-only" && entry.child_room_ids.length > 0 ? (
            <span className="space-members-meta" dir="auto">
              {childRoomContext(entry, childRoomLabels)}
            </span>
          ) : null}
        </span>
      </button>
      {sectionId === "child-only" ? (
        <button
          className="space-members-invite"
          type="button"
          aria-label={t("spaceMembers.invite")}
          disabled={inviteIsDisabled(state, entry, canInvite)}
          onClick={() => {
            onDiagnostic?.(
              `invite trigger=inline availability_reason=${inviteAvailabilityReasonForEntry(
                state,
                entry,
                canInvite,
                inviteAvailabilityReason
              )}`
            );
            onInviteUser(entry.user_id);
          }}
        >
          {entry.invite_pending ? t("spaceMembers.invitePending") : t("spaceMembers.invite")}
        </button>
      ) : null}
    </li>
  );
}
