import type {
  DesktopSnapshot,
  InviteHistoryPolicy,
  InviteScopeSelection,
  InviteTargetCandidate,
  InviteWorkflowState,
  RoomHistoryVisibility
} from "../../domain/types";

export const INVITE_ALREADY_IN_SPACE_MESSAGE = "既にスペースにいます";

export function buildFakeInviteHistoryPolicy(
  snapshot: DesktopSnapshot,
  roomId: string
): InviteHistoryPolicy {
  const room = snapshot.state.domain.rooms.find((entry) => entry.room_id === roomId);
  const settings = snapshot.state.domain.room_management.settings?.room_id === roomId
    ? snapshot.state.domain.room_management.settings
    : null;
  const recoveryRequired =
    Boolean(room?.is_encrypted) &&
    [
      "needsRecovery",
      "awaitingVerification",
      "verifying",
      "awaitingBootstrapConfirmation",
      "locked"
    ].includes(snapshot.state.domain.session.kind);
  return {
    current_visibility: settings?.history_visibility ?? ("joined" as RoomHistoryVisibility),
    encrypted: Boolean(room?.is_encrypted),
    can_edit: Boolean(settings?.permissions.can_edit_settings),
    readiness: recoveryRequired ? "recoveryRequired" : "ready"
  };
}

export function inviteScopeKey(scope: InviteScopeSelection): string {
  return scope.kind === "roomOnly" ? "roomOnly" : `parent:${scope.space_id}`;
}

export function defaultInviteWorkflowState(): InviteWorkflowState {
  return {
    query: {
      room_id: null,
      query: "",
      candidates: [],
      explicit_user_id: null
    },
    selected_targets: [],
    scope_plan: null,
    selected_scope: null,
    history_policy: null,
    operation: { kind: "idle" }
  };
}

export function buildFakeInviteScopePlan(
  snapshot: DesktopSnapshot,
  roomId: string
): InviteWorkflowState["scope_plan"] {
  if (snapshot.state.domain.spaces.some((space) => space.space_id === roomId)) {
    return {
      room_id: roomId,
      destination_kind: "space",
      default_scope: { kind: "roomOnly" },
      options: [{ scope: { kind: "roomOnly" }, label: "Space only", detail: null }]
    };
  }
  const room = snapshot.state.domain.rooms.find((candidate) => candidate.room_id === roomId);
  const activeSpaceId = snapshot.state.ui.navigation.active_space_id;
  const parentSpaceIds = room?.parent_space_ids ?? [];
  const orderedParentSpaceIds = [
    ...(activeSpaceId && parentSpaceIds.includes(activeSpaceId) ? [activeSpaceId] : []),
    ...parentSpaceIds.filter((spaceId) => spaceId !== activeSpaceId)
  ];
  const options: NonNullable<InviteWorkflowState["scope_plan"]>["options"] =
    orderedParentSpaceIds.map((spaceId) => {
    const space = snapshot.state.domain.spaces.find((candidate) => candidate.space_id === spaceId);
    return {
      scope: { kind: "parentSpaceAndRoom" as const, space_id: spaceId },
      label: `${space?.display_name ?? "Parent space"} and room`,
      detail: "Invite to the parent space before inviting to this room"
    };
  });
  options.push({ scope: { kind: "roomOnly" }, label: "Room only", detail: null });
  return {
    room_id: roomId,
    destination_kind: "room",
    default_scope: options[0]?.scope ?? { kind: "roomOnly" },
    options
  };
}

export function buildFakeInviteTargetQuery(
  snapshot: DesktopSnapshot,
  roomId: string,
  query: string
): InviteWorkflowState["query"] {
  const trimmed = query.trim();
  if (!trimmed) {
    return { room_id: roomId, query, candidates: [], explicit_user_id: null };
  }
  const lowered = trimmed.toLocaleLowerCase();
  const workflow = snapshot.state.domain.invite_workflow ?? defaultInviteWorkflowState();
  const selectedUserIds = new Set(
    workflow.selected_targets.map((target) => target.user_id)
  );
  const members = snapshot.state.domain.room_management.settings?.room_id === roomId
    ? snapshot.state.domain.room_management.settings.members
    : [];
  const destinationMembers = new Set(members.map((member) => member.user_id));
  const candidates = new Map<string, InviteTargetCandidate>();

  for (const [userId, profile] of Object.entries(snapshot.state.domain.profile.users)) {
    const alias = snapshot.state.domain.profile.local_aliases[userId] ?? null;
    if (
      fakeInviteTextMatches(userId, lowered) ||
      fakeInviteTextMatches(alias, lowered) ||
      fakeInviteTextMatches(profile.display_name, lowered) ||
      fakeInviteTextMatches(profile.display_label, lowered) ||
      profile.mention_search_terms.some((term) => fakeInviteTextMatches(term, lowered))
    ) {
      candidates.set(
        userId,
        fakeInviteCandidate(
          userId,
          (alias ?? profile.display_label) || userId,
          profile.original_display_label || profile.display_label || userId,
          profile.avatar,
          alias ? "localAlias" : "profile",
          selectedUserIds,
          destinationMembers
        )
      );
    }
  }

  for (const [userId, alias] of Object.entries(snapshot.state.domain.profile.local_aliases)) {
    if (!candidates.has(userId) && (fakeInviteTextMatches(userId, lowered) || fakeInviteTextMatches(alias, lowered))) {
      candidates.set(
        userId,
        fakeInviteCandidate(
          userId,
          alias,
          alias,
          null,
          "localAlias",
          selectedUserIds,
          destinationMembers
        )
      );
    }
  }

  for (const member of members) {
    const alias = snapshot.state.domain.profile.local_aliases[member.user_id] ?? null;
    if (
      !candidates.has(member.user_id) &&
      (fakeInviteTextMatches(member.user_id, lowered) ||
        fakeInviteTextMatches(alias, lowered) ||
        fakeInviteTextMatches(member.display_name, lowered) ||
        fakeInviteTextMatches(member.display_label, lowered))
    ) {
      candidates.set(
        member.user_id,
        fakeInviteCandidate(
          member.user_id,
          alias ?? member.display_label,
          member.original_display_label || member.display_label,
          null,
          "roomMember",
          selectedUserIds,
          destinationMembers
        )
      );
    }
  }

  const sortedCandidates = [...candidates.values()].sort((left, right) =>
    left.display_label.localeCompare(right.display_label) || left.user_id.localeCompare(right.user_id)
  );
  const explicit_user_id = trimmed.startsWith("@")
    ? fakeInviteCandidate(
        trimmed,
        trimmed,
        trimmed,
        null,
        "matrixId",
        new Set(),
        new Set(),
        fakeValidMatrixUserId(trimmed) ? "selectable" : "invalidMatrixId"
      )
    : null;
  return {
    room_id: roomId,
    query,
    candidates: sortedCandidates.slice(0, 8),
    explicit_user_id
  };
}

function fakeInviteCandidate(
  userId: string,
  displayLabel: string,
  originalDisplayLabel: string,
  avatar: InviteTargetCandidate["avatar"],
  source: InviteTargetCandidate["source"],
  selectedUserIds: Set<string>,
  destinationMembers: Set<string>,
  forcedStatus?: InviteTargetCandidate["status"]
): InviteTargetCandidate {
  return {
    user_id: userId,
    display_label: displayLabel,
    original_display_label: originalDisplayLabel,
    avatar,
    source,
    status:
      forcedStatus ??
      (selectedUserIds.has(userId)
        ? "alreadySelected"
        : destinationMembers.has(userId)
          ? "alreadyInDestination"
          : "selectable"),
    status_message: null
  };
}

function fakeInviteTextMatches(value: string | null | undefined, loweredQuery: string): boolean {
  return value?.toLocaleLowerCase().includes(loweredQuery) ?? false;
}

function fakeValidMatrixUserId(value: string): boolean {
  const match = value.match(/^@[^:\s]+:[^:\s]+$/);
  return match !== null;
}

export function fakeRoomHasMember(snapshot: DesktopSnapshot, roomId: string, userId: string): boolean {
  return (
    snapshot.state.domain.room_management.settings?.room_id === roomId &&
    snapshot.state.domain.room_management.settings.members.some((member) => member.user_id === userId)
  );
}
