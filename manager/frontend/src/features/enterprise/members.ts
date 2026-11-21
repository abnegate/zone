import { AuthRequestError } from '../../api/auth';
import { COMMUNITY_MEMBERS } from './limits';
import { MemberCapCode } from './MemberCapCode';
import type { Members } from './types';

export { COMMUNITY_MEMBERS };

const MEMBERS_UNCOUNTED = 'Member usage unavailable';

const ADD_LICENSE = 'new members are refused until a Team or Enterprise license is added';

const WITHOUT_LICENSE = 'without an active Zone Team or Enterprise license';

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
  return `This server allows ${COMMUNITY_MEMBERS} active ${membersNoun(COMMUNITY_MEMBERS)} ${WITHOUT_LICENSE}, and it has reached that limit. A server admin can add a license to make room for more people.`;
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
  return `This server has reached the ${members.cap} active ${membersNoun(members.cap)} it allows ${WITHOUT_LICENSE}, so ${ADD_LICENSE}. Everyone already here keeps access.`;
}

export function overMemberNotice({ active, cap, over }: Members): string | null {
  if (!over || active === null || cap === null) {
    return null;
  }
  const extra = active - cap;
  return `${active} people were active this month, ${extra} more than the ${cap} ${membersNoun(cap)} this server allows ${WITHOUT_LICENSE}. Everyone keeps access; ${ADD_LICENSE}.`;
}
