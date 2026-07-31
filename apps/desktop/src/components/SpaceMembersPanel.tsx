import { useMemo, useState } from "react";

import type { SpaceMemberEntry, SpaceMembersState } from "../domain/types";
import { t } from "../i18n/messages";
import { ImeTextField } from "./ImeTextControl";

export interface SpaceMembersPanelProps {
  state: SpaceMembersState;
  canInvite: boolean;
  onInviteUser: (userId: string) => void;
  onOpenProfile: (userId: string) => void;
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

export function SpaceMembersPanel({
  state,
  canInvite,
  onInviteUser,
  onOpenProfile
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
  const hasResults = filteredSections.some((section) => section.entries.length > 0);

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
                  <li className="space-members-row" data-user-id={entry.user_id} key={entry.user_id}>
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
                            {t("spaceMembers.childRoomContext", {
                              rooms: entry.child_room_ids.join(", ")
                            })}
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
                        onClick={() => onInviteUser(entry.user_id)}
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
