import { describe, expect, it } from 'bun:test';
import {
  legacyOrganizationPath,
  legacyWorkspacePath,
  relocate,
  SESSIONS_PATH,
  SOURCES_PATH,
} from './legacy';

const ORGANIZATION_TABS = [
  ['ai', '/settings/organization/ai'],
  ['members', '/settings/organization/members'],
  ['invitations', '/settings/organization/members'],
  ['devices', '/settings/organization/devices'],
  ['billing', '/settings/organization/billing'],
  ['audit', '/settings/organization/audit'],
  ['github', '/settings/organization/github'],
] as const;

const WORKSPACE_TABS = [
  ['theme', '/settings/workspace/general'],
  ['ai', '/settings/workspace/ai'],
  ['features', '/settings/server/features'],
  ['folders', '/settings/workspace/folders'],
  ['members', '/settings/workspace/members'],
  ['devices', '/settings/server/connect'],
] as const;

describe('legacyOrganizationPath', () => {
  it('maps every old organization settings tab to its section', () => {
    for (const [tab, path] of ORGANIZATION_TABS) {
      expect(legacyOrganizationPath(`?tab=${tab}`, ''), tab).toBe(path);
    }
  });

  it('drops the tab and keeps the other params and the hash', () => {
    expect(legacyOrganizationPath('?tab=invitations&x=1', '#y')).toBe(
      '/settings/organization/members?x=1#y'
    );
    expect(legacyOrganizationPath('?a=1&tab=billing&b=2', '')).toBe(
      '/settings/organization/billing?a=1&b=2'
    );
  });

  it("keeps a GitHub callback's outcome and organization on the GitHub section", () => {
    expect(legacyOrganizationPath('?tab=github&github=installed&organization=org-2', '')).toBe(
      '/settings/organization/github?github=installed&organization=org-2'
    );
  });

  it('lands on AI defaults without a tab', () => {
    expect(legacyOrganizationPath('', '')).toBe('/settings/organization/ai');
    expect(legacyOrganizationPath('?x=1', '')).toBe('/settings/organization/ai?x=1');
    expect(legacyOrganizationPath('', '#y')).toBe('/settings/organization/ai#y');
  });

  it('lands on AI defaults for an unknown tab', () => {
    expect(legacyOrganizationPath('?tab=nothing', '')).toBe('/settings/organization/ai');
    expect(legacyOrganizationPath('?tab=theme&x=1', '#y')).toBe('/settings/organization/ai?x=1#y');
  });
});

describe('legacyWorkspacePath', () => {
  it('maps every old workspace settings tab to its section', () => {
    for (const [tab, path] of WORKSPACE_TABS) {
      expect(legacyWorkspacePath(`?tab=${tab}`, ''), tab).toBe(path);
    }
  });

  it('drops the tab and keeps the other params and the hash', () => {
    expect(legacyWorkspacePath('?tab=ai&x=1', '#y')).toBe('/settings/workspace/ai?x=1#y');
    expect(legacyWorkspacePath('?a=1&tab=folders&b=2', '')).toBe(
      '/settings/workspace/folders?a=1&b=2'
    );
    expect(legacyWorkspacePath('?tab=devices', '#pair')).toBe('/settings/server/connect#pair');
  });

  it('has no legacy path without a tab', () => {
    expect(legacyWorkspacePath('', '')).toBeNull();
    expect(legacyWorkspacePath('?x=1', '#y')).toBeNull();
  });

  it('has no legacy path for an unknown tab', () => {
    expect(legacyWorkspacePath('?tab=nothing', '')).toBeNull();
    expect(legacyWorkspacePath('?tab=billing', '')).toBeNull();
  });
});

describe('sessions path', () => {
  it('points the old sessions page at You › Sessions', () => {
    expect(SESSIONS_PATH).toBe('/settings/you/sessions');
  });
});

describe('sources path', () => {
  it('points the old sources page at Workspace › Sources', () => {
    expect(SOURCES_PATH).toBe('/settings/workspace/sources');
  });
});

describe('relocate', () => {
  it('moves to the path keeping the query and the hash', () => {
    expect(relocate('/settings/you/sessions', '?x=1&y=2', '#z')).toBe(
      '/settings/you/sessions?x=1&y=2#z'
    );
    expect(relocate('/settings/you/sessions', '', '#z')).toBe('/settings/you/sessions#z');
    expect(relocate('/settings/you/sessions', '', '')).toBe('/settings/you/sessions');
  });

  it('drops the legacy tab', () => {
    expect(relocate('/settings/workspace/general', '?tab=nothing&x=1', '#y')).toBe(
      '/settings/workspace/general?x=1#y'
    );
    expect(relocate('/settings/workspace/general', '?tab=nothing', '')).toBe(
      '/settings/workspace/general'
    );
  });
});
