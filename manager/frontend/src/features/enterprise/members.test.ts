import { describe, expect, it } from 'bun:test';
import { AuthRequestError } from '../../api/auth';
import { capped, uncapped } from './fixtures';
import {
  COMMUNITY_MEMBERS,
  isMemberCapRefusal,
  memberCapMessage,
  memberCapNotice,
  memberText,
  overMemberNotice,
} from './members';
import { MemberCapCode } from './schemas';

const LOSS = /block|lock|suspend|disable|lose|remov/i;

describe('isMemberCapRefusal', () => {
  it('recognises the refusal the server sends at the cap', () => {
    expect(isMemberCapRefusal(new AuthRequestError('Full', MemberCapCode))).toBe(true);
  });

  it('ignores every other refusal and plain errors', () => {
    expect(isMemberCapRefusal(new AuthRequestError('Required', 'sso_required'))).toBe(false);
    expect(isMemberCapRefusal(new Error(MemberCapCode))).toBe(false);
    expect(isMemberCapRefusal({ code: MemberCapCode })).toBe(false);
    expect(isMemberCapRefusal(null)).toBe(false);
  });
});

describe('memberCapMessage', () => {
  it('names the Community cap and the plans that lift it', () => {
    const message = memberCapMessage();
    expect(message).toContain(`allows ${COMMUNITY_MEMBERS} active members`);
    expect(message).toContain('Zone Team or Enterprise license');
    expect(message).toMatch(/^[A-Z].*\.$/);
    expect(message).not.toMatch(LOSS);
  });

  it('caps Community at three members', () => {
    expect(COMMUNITY_MEMBERS).toBe(3);
  });
});

describe('memberText', () => {
  it('counts active members against the cap', () => {
    expect(memberText(capped(2))).toBe('2 of 3 members this month');
    expect(memberText(capped(5))).toBe('5 of 3 members this month');
  });

  it('names a single allowed member in the singular', () => {
    expect(memberText(capped(0, 1))).toBe('0 of 1 member this month');
  });

  it('says usage is unavailable rather than inventing a count, keeping the cap', () => {
    expect(memberText(capped(null))).toBe('Member usage unavailable (3 members allowed)');
  });

  it('has nothing to say for an uncapped server', () => {
    expect(memberText(uncapped(40))).toBeNull();
    expect(memberText(uncapped(null))).toBeNull();
  });
});

describe('memberCapNotice', () => {
  it('says new members are refused once the cap is reached', () => {
    const notice = memberCapNotice(capped(3));
    expect(notice).toContain('reached the 3 active members Zone Community allows');
    expect(notice).toContain('new members are refused until a Team or Enterprise license is added');
    expect(notice).toContain('Everyone already here keeps access.');
    expect(notice).not.toMatch(LOSS);
  });

  it('says nothing under the cap, over it, uncapped or uncounted', () => {
    expect(memberCapNotice(capped(2))).toBeNull();
    expect(memberCapNotice(capped(4))).toBeNull();
    expect(memberCapNotice(uncapped(40))).toBeNull();
    expect(memberCapNotice(capped(null))).toBeNull();
  });
});

describe('overMemberNotice', () => {
  it('counts the extra people and promises nobody loses access', () => {
    const notice = overMemberNotice(capped(5));
    expect(notice).toContain(
      '5 people were active this month, 2 more than the 3 members Zone Community allows'
    );
    expect(notice).toContain('Everyone keeps access');
    expect(notice).toContain('new members are refused until a Team or Enterprise license is added');
    expect(notice).not.toMatch(LOSS);
  });

  it('says nothing at or under the cap, or uncapped', () => {
    expect(overMemberNotice(capped(3))).toBeNull();
    expect(overMemberNotice(capped(1))).toBeNull();
    expect(overMemberNotice(uncapped(40))).toBeNull();
  });

  it('says nothing when usage could not be counted, even if the response claims over', () => {
    expect(overMemberNotice({ active: null, cap: 3, over: true })).toBeNull();
    expect(overMemberNotice({ active: 5, cap: null, over: true })).toBeNull();
  });
});
