import { useEffect, useMemo, useState } from "react";

import type { SpaceMemberEntry, SpaceMembersState } from "../domain/types";
import { contextMenuItems } from "../domain/contextMenus";
import { t } from "../i18n/messages";
import type { OpenContextMenu } from "../app/uiShared";
import { ImeTextField } from "./ImeTextControl";

export type SpaceInviteAvailabilityReason =
  | "available"
  | "settings_unavailable"
  | "permission_denied"
  | "operation_pending"
  | "invite_pending";

export interface SpaceMembersPanelProps {
  state: SpaceMembersState;
  canInvite: boolean;
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
  return state.operation.kind === "loading" || state.operation.kind === "inviting";
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
  childRoomLabels = new Map<string, string>(),
  onInviteUser,
  onOpenProfile,
  onOpenContextMenu,
  onDiagnostic,
  inviteAvailabilityReason
}: SpaceMembersPanelProps) {
  const [query, setQuery] = useState("");
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
    <section className="space-members-panel" aria-labelledby="space-members-title">
      <header className="space-members-header">
        <h2 id="space-members-title">{t("spaceMembers.title")}</h2>
        <span className="space-members-count" aria-label={t("spaceMembers.joinedCount", {
          count: state.space_joined.length
        })}>
          {state.space_joined.length}
        </span>
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
          {t("spaceMembers.inviteFailed")}
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
                  <li
                    className="space-members-row"
                    data-user-id={entry.user_id}
                    key={entry.user_id}
                    onContextMenu={(event) => {
                      if (
                        section.id !== "child-only" ||
                        !onOpenContextMenu ||
                        !state.selected_space_id
                      ) {
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
                      <span className="space-members-avatar" aria-hidden="true">
                        {memberInitials(entry)}
                      </span>
                      <span className="space-members-row-text">
                        <span className="space-members-name" dir="auto">
                          {entry.display_label}
                        </span>
                        {section.id === "child-only" && entry.child_room_ids.length > 0 ? (
                          <span className="space-members-meta" dir="auto">
                            {childRoomContext(entry, childRoomLabels)}
                          </span>
                        ) : null}
                      </span>
                    </button>
                    {section.id === "child-only" ? (
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
                        {entry.invite_pending
                          ? t("spaceMembers.invitePending")
                          : t("spaceMembers.invite")}
                      </button>
                    ) : null}
                  </li>
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
