export interface LabelledMember {
  user_id: string;
  email: string;
  display_name: string | null;
}

const ID_PREFIX_LENGTH = 8;

export const memberLabel = (member: LabelledMember): string =>
  member.display_name || member.email || member.user_id.slice(0, ID_PREFIX_LENGTH);
