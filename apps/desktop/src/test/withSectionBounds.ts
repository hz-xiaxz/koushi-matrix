import type { DesktopSnapshot, SidebarModel } from "../domain/types";
import { sidebarSectionBoundsForFake } from "./sidebarSectionBoundsFixture";

/**
 * #1363/#1366: attach the Rust-projected row bounds to a fake snapshot.
 *
 * The numbers come from `sidebarSectionBounds.generated.json`, which the
 * `koushi-state` fixture test writes and asserts against
 * `SidebarSectionBound::project`. The fake only picks the case its own stored
 * settings and navigation correspond to; it never computes a bound itself.
 *
 * A browser spec may install a partial snapshot: the signed-out auth flow
 * replaces `sidebar` with a literal that carries no `sections` projection at
 * all (`e2e/profile-settings-session.spec.ts`). Such a snapshot has no sidebar
 * to bound, so it passes through untouched instead of throwing.
 */
export function withSectionBounds(snapshot: DesktopSnapshot): DesktopSnapshot {
  const sidebar: Partial<SidebarModel> | undefined = snapshot.sidebar;
  const sections = sidebar?.sections;
  if (!sections) {
    return snapshot;
  }
  const state: Partial<DesktopSnapshot["state"]> = snapshot.state;
  const navigation = state.ui?.navigation;
  const scope =
    state.domain?.settings?.values.sidebar.scope_preferences?.[
      navigation?.active_space_id ?? "__home__"
    ];
  return {
    ...snapshot,
    sidebar: {
      ...snapshot.sidebar,
      section_bounds: sidebarSectionBoundsForFake({
        sections: {
          rooms: sections.rooms,
          people: sections.people,
          low_priority: sections.low_priority
        },
        expanded: {
          rooms: scope?.rooms?.expanded ?? false,
          dms: scope?.dms?.expanded ?? false,
          lowPriority: scope?.low_priority?.expanded ?? false
        },
        activeRoomId: navigation?.active_room_id ?? null
      })
    }
  };
}
