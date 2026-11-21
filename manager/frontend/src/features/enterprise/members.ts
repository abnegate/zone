import { AuthRequestError } from '../../api/auth';
import { MemberCapCode } from './MemberCapCode';
import type { Members } from './types';

export const COMMUNITY_MEMBERS = 3;

const MEMBERS_UNCOUNTED = 'Member usage unavailable';

const ADD_LICENSE = 'new members are refused until a Team or Enterprise license is added';

function membersNoun(count: number): string {
  return count === 1 ? 'member' : 'members';
}

function full({ active, cap }: Members): boolean {
  return active !== null && cap !== null && active >= cap;
}

export function isMemberCapRefusal(error: unknown): error is AuthRequestError {
  return error instanceof AuthRequestError && error.code === MemberCapCode;
}

export function memberCapMessage(): string {
  return `This server runs Zone Community, which allows ${COMMUNITY_MEMBERS} active members, and it has reached that limit. A server admin can add a Zone Team or Enterprise license to make room for more people.`;
}

export function memberText({ active, cap }: Members): string | null {
  if (cap === null) {
    return null;
  }
  if (active === null) {
    return `${MEMBERS_UNCOUNTED} (${cap} ${membersNoun(cap)} allowed)`;
  }
  return `${active} of ${cap} ${membersNoun(cap)} this month`;
}

export function memberCapNotice(members: Members): string | null {
  if (members.over || !full(members) || members.cap === null) {
    return null;
  }
  return `This server has reached the ${members.cap} active ${membersNoun(members.cap)} Zone Community allows, so ${ADD_LICENSE}. Everyone already here keeps access.`;
}

export function overMemberNotice({ active, cap, over }: Members): string | null {
  if (!over || active === null || cap === null) {
    return null;
  }
  const extra = active - cap;
  return `${active} people were active this month, ${extra} more than the ${cap} ${membersNoun(cap)} Zone Community allows. Everyone keeps access; ${ADD_LICENSE}.`;
}
