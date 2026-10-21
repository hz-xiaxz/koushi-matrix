import type { AvatarImage, MentionCandidateMembership, MentionTarget } from "./types";

export type MentionCandidate = {
  key: string;
  label: string;
  avatar?: AvatarImage | null;
  membership?: MentionCandidateMembership;
  target: MentionTarget;
};

export type TimelineForwardDestination = {
  room_id: string;
  display_name: string;
};
