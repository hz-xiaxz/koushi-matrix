import type {
  DesktopSnapshot,
  InvitePreview,
  ProfileState,
  RoomListItem,
  RoomListProjection,
  RoomListSections,
  RoomNotificationSettings,
  RoomSummary,
  SearchScopeKind,
  SpaceSummary,
  VisibleRooms
} from "./types";

export function visibleRooms(snapshot: DesktopSnapshot): VisibleRooms {
  return {
    spaceRooms: snapshot.sidebar.space_rooms,
    globalDms: snapshot.sidebar.global_dms
  };
}

export function projectRoomSummaries(
  rooms: RoomSummary[],
  profile: ProfileState
): RoomSummary[] {
  return rooms.map((room) => projectRoomSummary(room, profile));
}

function projectRoomSummary(room: RoomSummary, profile: ProfileState): RoomSummary {
  if (!room.is_dm || room.dm_user_ids.length !== 1) {
    return room;
  }
  const user = profile.users[room.dm_user_ids[0] ?? ""];
  if (!user) {
    return room;
  }
  const displayLabel = user.display_label.trim() || room.display_label;
  const originalDisplayLabel = user.original_display_label.trim() || room.original_display_label;
  const avatar = user.avatar ?? room.avatar;
  if (
    displayLabel === room.display_label &&
    originalDisplayLabel === room.original_display_label &&
    avatar === room.avatar
  ) {
    return room;
  }
  return {
    ...room,
    display_label: displayLabel,
    original_display_label: originalDisplayLabel,
    avatar
  };
}

export function roomListSections(
  roomList: RoomListProjection,
  activeSpaceId: string | null,
  spaces: SpaceSummary[],
  rooms: RoomSummary[],
  invites: InvitePreview[],
  roomNotificationSettings: Record<string, RoomNotificationSettings> = {}
): RoomListSections {
  const fullSections = roomListSectionsFromSidebar(
    activeSpaceId,
    spaces,
    rooms,
    roomNotificationSettings
  );
  if (roomList.items === null) {
    return fullSections;
  }

  const projectedSections = roomListSectionsFromProjection(
    roomList.items,
    activeSpaceId,
    spaces,
    rooms,
    invites,
    roomNotificationSettings
  );
  switch (roomList.active_filter.kind) {
    case "rooms":
      return { ...fullSections, rooms: projectedSections.rooms };
    case "people":
      return { ...fullSections, people: projectedSections.people };
    case "favourites":
      return { ...fullSections, favourites: projectedSections.favourites };
    case "invites":
      return { ...fullSections, invites: projectedSections.invites };
    case "unread":
      return projectedSections;
  }
}

function roomListSectionsFromSidebar(
  activeSpaceId: string | null,
  spaces: SpaceSummary[],
  rooms: RoomSummary[],
  roomNotificationSettings: Record<string, RoomNotificationSettings>
): RoomListSections {
  const sidebar = composeSidebar(activeSpaceId, spaces, rooms, roomNotificationSettings);
  return {
    favourites: sidebar.space_rooms.filter((room) => room.tags.favourite !== null),
    invites: [],
    notJoined: sidebar.not_joined_space_rooms,
    rooms: sidebar.space_rooms.filter(
      (room) => room.tags.favourite === null && room.tags.low_priority === null
    ),
    people: sidebar.global_dms,
    lowPriority: sidebar.space_rooms.filter((room) => room.tags.low_priority !== null)
  };
}

function roomListSectionsFromProjection(
  items: NonNullable<RoomListProjection["items"]>,
  activeSpaceId: string | null,
  spaces: SpaceSummary[],
  rooms: RoomSummary[],
  invites: InvitePreview[],
  roomNotificationSettings: Record<string, RoomNotificationSettings>
): RoomListSections {
  const roomById = new Map(rooms.map((room) => [room.room_id, room]));
  const inviteById = new Map(invites.map((invite) => [invite.room_id, invite]));
  const activeSpace = activeSpaceId
    ? spaces.find((space) => space.space_id === activeSpaceId) ?? null
    : null;

  const favourites: RoomListItem[] = [];
  const invitesSection: RoomListItem[] = [];
  const notJoined: RoomListItem[] = [];
  const roomsSection: RoomListItem[] = [];
  const people: RoomListItem[] = [];
  const lowPriority: RoomListItem[] = [];

  for (const item of items) {
    const room = item.kind === "room" ? roomById.get(item.room_id) : undefined;
    const invite = item.kind === "invite" ? inviteById.get(item.room_id) : undefined;
    const source = room ?? invite;
    if (!source) {
      continue;
    }
    if (!room && !invite) {
      continue;
    }
    if (invite) {
      const listItem = inviteListItem(invite);
      if (invite.is_dm) {
        people.push(listItem);
      } else {
        invitesSection.push(listItem);
      }
      continue;
    }

    if (!room) {
      continue;
    }
    if (!roomVisibleInSidebarScope(room, activeSpaceId, activeSpace)) {
      continue;
    }

    const listItem = roomListItem(room, roomNotificationSettings);
    if (room.is_dm) {
      people.push(listItem);
    } else if (room.tags.favourite !== null) {
      favourites.push(listItem);
    } else if (room.tags.low_priority !== null) {
      lowPriority.push(listItem);
    } else {
      roomsSection.push(listItem);
    }
  }

  return {
    favourites,
    invites: invitesSection,
    notJoined,
    rooms: roomsSection,
    people,
    lowPriority
  };
}

function roomVisibleInSidebarScope(
  room: RoomSummary,
  activeSpaceId: string | null,
  activeSpace: SpaceSummary | null
): boolean {
  if (activeSpaceId === null) {
    return true;
  }
  if (room.is_dm) {
    return room.dm_space_ids.includes(activeSpaceId);
  }
  return (
    room.parent_space_ids.includes(activeSpaceId) ||
    (activeSpace?.child_room_ids.includes(room.room_id) ?? false)
  );
}

function inviteListItem(invite: InvitePreview): RoomListItem {
  return {
    room_id: invite.room_id,
    display_name: invite.display_name,
    avatar: invite.avatar,
    tags: { favourite: null, low_priority: null },
    unread_count: 0,
    highlight_count: 0
  };
}

export function composeSidebar(
  activeSpaceId: string | null,
  spaces: SpaceSummary[],
  rooms: RoomSummary[],
  roomNotificationSettings: Record<string, RoomNotificationSettings> = {},
  // Mirrors the Rust `compose_sidebar` wrapper: a caller with only rooms and
  // spaces reports no pending invites rather than guessing (#330).
  pendingInviteCount = 0
): DesktopSnapshot["sidebar"] {
  const roomById = new Map(rooms.map((room) => [room.room_id, room]));
  const roomIsMuted = (roomId: string) =>
    roomNotificationSettings[roomId]?.mode.kind === "mute";
  const aggregateRooms = rooms.filter((room) => !roomIsMuted(room.room_id));
  const homeUnreadCount = aggregateRooms.reduce(
    (sum, room) => sum + roomActivityUnreadCount(room),
    0
  );
  const aggregateRoomItems = (items: RoomListItem[]) =>
    items.filter((room) => !roomIsMuted(room.room_id));
  const activeSpace = activeSpaceId
    ? spaces.find((space) => space.space_id === activeSpaceId) ?? null
    : null;
  const roomExists = (room: RoomSummary | undefined): room is RoomSummary =>
    room !== undefined && !room.is_dm;

  const spaceRooms = activeSpaceId
    ? activeSpace
        ?.child_room_ids.map((roomId) => roomById.get(roomId))
        .filter(roomExists)
        .map((room) => roomListItem(room, roomNotificationSettings)) ?? []
    : rooms
        .filter((room) => !room.is_dm)
        .map((room) => roomListItem(room, roomNotificationSettings));

  const notJoinedSpaceRooms: RoomListItem[] = [];

  const globalDms = rooms
    .filter(
      (room) =>
        room.is_dm && (activeSpaceId === null || room.dm_space_ids.includes(activeSpaceId))
    )
    .map((room) => roomListItem(room, roomNotificationSettings));

  return {
    active_space_id: activeSpaceId,
    account_home: {
      display_name: "Home",
      unread_count: homeUnreadCount,
      highlight_count: aggregateRooms.reduce(
        (sum, room) => sum + (room.highlight_count ?? 0),
        0
      ),
      invite_count: pendingInviteCount,
      attention_count: homeUnreadCount + pendingInviteCount,
      is_active: activeSpaceId === null
    },
    space_rail: spaces.map((space) => ({
      space_id: space.space_id,
      display_name: space.display_name,
      avatar: space.avatar,
      unread_count: space.child_room_ids
        .map((roomId) => rooms.find((room) => room.room_id === roomId))
        .filter(roomExists)
        .filter((room) => !roomIsMuted(room.room_id))
        .reduce((sum, room) => sum + roomActivityUnreadCount(room), 0),
      highlight_count: space.child_room_ids
        .map((roomId) => rooms.find((room) => room.room_id === roomId))
        .filter(roomExists)
        .filter((room) => !roomIsMuted(room.room_id))
        .reduce((sum, room) => sum + (room.highlight_count ?? 0), 0),
      is_active: space.space_id === activeSpaceId
    })),
    space_rooms: spaceRooms,
    not_joined_space_rooms: notJoinedSpaceRooms,
    global_dms: globalDms,
    space_unread_count: aggregateRoomItems(spaceRooms).reduce(
      (sum, room) => sum + room.unread_count,
      0
    ),
    dm_unread_count: aggregateRoomItems(globalDms).reduce(
      (sum, room) => sum + room.unread_count,
      0
    ),
    space_highlight_count: aggregateRoomItems(spaceRooms).reduce(
      (sum, room) => sum + room.highlight_count,
      0
    ),
    dm_highlight_count: aggregateRoomItems(globalDms).reduce(
      (sum, room) => sum + room.highlight_count,
      0
    )
  };
}

export function roomIsInScope(
  roomId: string,
  scope: SearchScopeKind,
  snapshot: DesktopSnapshot
): boolean {
  switch (scope) {
    case "currentRoom":
      return snapshot.state.ui.navigation.active_room_id === roomId;
    case "currentSpace": {
      const activeSpace = snapshot.state.domain.spaces.find(
        (space) => space.space_id === snapshot.state.ui.navigation.active_space_id
      );
      return Boolean(activeSpace?.child_room_ids.includes(roomId));
    }
    case "allRooms":
      return true;
  }
}

export function textRangeUtf16(haystack: string, needle: string) {
  const start = haystack.indexOf(needle);
  if (start < 0 || needle.length === 0) {
    return null;
  }
  const end = start + needle.length;
  return {
    start_utf16: utf16Length(haystack.slice(0, start)),
    end_utf16: utf16Length(haystack.slice(0, end))
  };
}

function roomListItem(
  room: RoomSummary,
  roomNotificationSettings: Record<string, RoomNotificationSettings> = {}
): RoomListItem {
  const mode = roomNotificationSettings[room.room_id]?.mode.kind;
  const isMuted = mode === "mute";
  const hasUnreadContent =
    room.unread_count > 0 ||
    (room.notification_count ?? 0) > 0 ||
    (room.highlight_count ?? 0) > 0 ||
    Boolean(room.marked_unread);
  const unreadCount = roomActivityUnreadCount(room);
  const hasUnreadMention = !isMuted && (room.highlight_count ?? 0) > 0;
  const isAttentionHighlighted =
    !isMuted &&
    ((room.notification_count ?? 0) > 0 ||
      (room.highlight_count ?? 0) > 0 ||
      Boolean(room.marked_unread));
  const notificationCount =
    isMuted || (mode === "mentions" && (room.highlight_count ?? 0) === 0)
      ? 0
      : room.notification_count ?? room.unread_count;
  return {
    room_id: room.room_id,
    display_name: room.display_label ?? room.display_name,
    avatar: room.avatar,
    tags: room.tags,
    unread_count: unreadCount,
    highlight_count: isMuted ? 0 : room.highlight_count ?? 0,
    notification_count: notificationCount,
    display_count: isMuted ? room.unread_count : notificationCount,
    has_unread_content: hasUnreadContent,
    is_attention_highlighted: isAttentionHighlighted,
    has_unread_mention: hasUnreadMention,
    is_muted: isMuted
  };
}

function roomActivityUnreadCount(room: RoomSummary): number {
  const notificationCount = room.notification_count ?? 0;
  const highlightCount = room.highlight_count ?? 0;
  const count = Math.max(room.unread_count, notificationCount, highlightCount);
  if (count > 0) {
    return count;
  }
  return room.marked_unread ? 1 : 0;
}

function utf16Length(value: string): number {
  return [...value].reduce((sum, char) => sum + char.length, 0);
}
