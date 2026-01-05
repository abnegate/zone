import type { SourceCategory, SourceType } from '../types';
import {
  getEnabledSources,
  getSourceBadgeColor,
  getSourceById,
  getSourceLabel,
  getSourcesByCategory,
  initializeFormState,
  sourceRegistry,
} from './index';

describe('Source Registry', () => {
  describe('sourceRegistry', () => {
    it('contains all expected source types', () => {
      const expectedIds: SourceType[] = [
        'github',
        'gitlab',
        'filesystem',
        'ical',
        'imap',
        'web',
        'text',
        'discord',
        'slack',
      ];

      const registryIds = sourceRegistry.map((s) => s.id);

      for (const id of expectedIds) {
        expect(registryIds).toContain(id);
      }
    });

    it('all sources have required properties', () => {
      for (const source of sourceRegistry) {
        expect(source.id).toBeDefined();
        expect(source.name).toBeDefined();
        expect(source.category).toBeDefined();
        expect(source.description).toBeDefined();
        expect(source.icon).toBeDefined();
        expect(source.badgeColor).toBeDefined();
        expect(source.iconWrapperClass).toBeDefined();
        expect(typeof source.enabled).toBe('boolean');
        expect(source.formFields).toBeInstanceOf(Array);
        expect(typeof source.buildConfig).toBe('function');
        expect(typeof source.getDefaultName).toBe('function');
        expect(typeof source.getFieldIds).toBe('function');
      }
    });
  });

  describe('getSourceById', () => {
    it('returns source for valid id', () => {
      const source = getSourceById('github');
      expect(source).toBeDefined();
      expect(source?.id).toBe('github');
      expect(source?.name).toBe('GitHub');
    });

    it('returns undefined for invalid id', () => {
      const source = getSourceById('invalid' as SourceType);
      expect(source).toBeUndefined();
    });
  });

  describe('getSourcesByCategory', () => {
    it('returns file sources', () => {
      const fileSources = getSourcesByCategory('file');
      expect(fileSources.length).toBeGreaterThan(0);

      for (const source of fileSources) {
        expect(source.category).toBe('file');
      }
    });

    it('returns mail sources', () => {
      const mailSources = getSourcesByCategory('mail');
      expect(mailSources.length).toBeGreaterThan(0);

      for (const source of mailSources) {
        expect(source.category).toBe('mail');
      }
    });

    it('returns empty array for non-existent category', () => {
      const sources = getSourcesByCategory('nonexistent' as SourceCategory);
      expect(sources).toHaveLength(0);
    });
  });

  describe('getEnabledSources', () => {
    it('returns only enabled sources', () => {
      const enabled = getEnabledSources();

      for (const source of enabled) {
        expect(source.enabled).toBe(true);
      }
    });

    it('does not include disabled sources', () => {
      const enabled = getEnabledSources();
      const enabledIds = enabled.map((s) => s.id);

      // Discord and Slack are disabled
      expect(enabledIds).not.toContain('discord');
      expect(enabledIds).not.toContain('slack');
    });
  });

  describe('getSourceBadgeColor', () => {
    it('returns correct badge color for known type', () => {
      const color = getSourceBadgeColor('github');
      expect(color).toBe('badge-purple');
    });

    it('returns default color for unknown type', () => {
      const color = getSourceBadgeColor('unknown' as SourceType);
      expect(color).toBe('badge-gray');
    });
  });

  describe('getSourceLabel', () => {
    it('returns correct label for known type', () => {
      expect(getSourceLabel('github')).toBe('GitHub');
      expect(getSourceLabel('gitlab')).toBe('GitLab');
      expect(getSourceLabel('filesystem')).toBe('Filesystem');
    });

    it('returns type as label for unknown type', () => {
      const label = getSourceLabel('unknown' as SourceType);
      expect(label).toBe('unknown');
    });
  });

  describe('initializeFormState', () => {
    it('initializes github form state with defaults', () => {
      const state = initializeFormState('github');

      expect(state.ghOwner).toBe('');
      expect(state.ghRepo).toBe('');
      expect(state.ghBranch).toBe('main');
      expect(state.credentials).toBe('');
    });

    it('initializes gitlab form state with defaults', () => {
      const state = initializeFormState('gitlab');

      expect(state.glHost).toBe('https://gitlab.com');
      expect(state.glProjectId).toBe('');
      expect(state.glBranch).toBe('main');
    });

    it('initializes filesystem form state', () => {
      const state = initializeFormState('filesystem');

      expect(state.fsBasePath).toBe('');
      expect(state.fsAllowWrites).toBe(true);
    });

    it('initializes web form state', () => {
      const state = initializeFormState('web');

      expect(state.webUrl).toBe('');
    });

    it('initializes imap form state with defaults', () => {
      const state = initializeFormState('imap');

      expect(state.imapHost).toBe('');
      expect(state.imapPort).toBe(993);
      expect(state.imapUsername).toBe('');
      expect(state.imapFolder).toBe('INBOX');
      expect(state.imapUseSsl).toBe(true);
    });

    it('returns empty object for unknown source', () => {
      const state = initializeFormState('unknown' as SourceType);
      expect(state).toEqual({});
    });
  });
});

describe('Individual Source Definitions', () => {
  describe('GitHub Source', () => {
    const github = getSourceById('github')!;

    it('builds config correctly', () => {
      const config = github.buildConfig({
        ghOwner: 'myorg',
        ghRepo: 'myrepo',
        ghBranch: 'develop',
      });

      expect(config).toEqual({
        owner: 'myorg',
        repo: 'myrepo',
        branch: 'develop',
      });
    });

    it('uses default branch when not specified', () => {
      const config = github.buildConfig({
        ghOwner: 'myorg',
        ghRepo: 'myrepo',
        ghBranch: '',
      });

      expect(config).toEqual({
        owner: 'myorg',
        repo: 'myrepo',
        branch: 'main',
      });
    });

    it('generates default name', () => {
      const name = github.getDefaultName({
        ghOwner: 'myorg',
        ghRepo: 'myrepo',
      });

      expect(name).toBe('myorg/myrepo');
    });

    it('returns field IDs', () => {
      const ids = github.getFieldIds();
      expect(ids).toContain('ghOwner');
      expect(ids).toContain('ghRepo');
      expect(ids).toContain('ghBranch');
    });
  });

  describe('GitLab Source', () => {
    const gitlab = getSourceById('gitlab')!;

    it('builds config correctly', () => {
      const config = gitlab.buildConfig({
        glProjectId: 'mygroup/myproject',
        glHost: 'https://gitlab.example.com',
        glBranch: 'main',
      });

      expect(config).toEqual({
        project_id: 'mygroup/myproject',
        host: 'https://gitlab.example.com',
        branch: 'main',
      });
    });

    it('generates default name', () => {
      const name = gitlab.getDefaultName({
        glProjectId: 'mygroup/myproject',
      });

      expect(name).toBe('mygroup/myproject');
    });
  });

  describe('Filesystem Source', () => {
    const fs = getSourceById('filesystem')!;

    it('builds config correctly', () => {
      const config = fs.buildConfig({
        fsBasePath: '/path/to/folder',
        fsAllowWrites: true,
      });

      expect(config).toEqual({
        base_path: '/path/to/folder',
        allow_writes: true,
      });
    });

    it('generates default name from path', () => {
      const name = fs.getDefaultName({
        fsBasePath: '/home/user/documents',
      });

      expect(name).toBe('documents');
    });
  });

  describe('Web Source', () => {
    const web = getSourceById('web')!;

    it('builds config correctly', () => {
      const config = web.buildConfig({
        webUrl: 'https://example.com',
      });

      expect(config).toEqual({
        url: 'https://example.com',
      });
    });

    it('generates default name from URL hostname', () => {
      const name = web.getDefaultName({
        webUrl: 'https://example.com/docs',
      });

      expect(name).toBe('example.com');
    });

    it('returns fallback name for invalid URL', () => {
      const name = web.getDefaultName({
        webUrl: 'not-a-url',
      });

      expect(name).toBe('Web URL');
    });
  });

  describe('IMAP Source', () => {
    const imap = getSourceById('imap')!;

    it('builds config correctly', () => {
      const config = imap.buildConfig({
        imapHost: 'imap.example.com',
        imapPort: 993,
        imapUsername: 'user@example.com',
        imapUseSsl: true,
        imapFolder: 'INBOX',
      });

      expect(config).toEqual({
        host: 'imap.example.com',
        port: 993,
        username: 'user@example.com',
        use_ssl: true,
        folder: 'INBOX',
      });
    });

    it('generates default name', () => {
      const name = imap.getDefaultName({
        imapUsername: 'user@example.com',
        imapHost: 'imap.gmail.com',
      });

      expect(name).toBe('user@example.com@imap.gmail.com');
    });
  });

  describe('Text Source', () => {
    const text = getSourceById('text')!;

    it('builds config correctly', () => {
      const config = text.buildConfig({
        textContent: 'Some important text content',
        textLabel: 'My notes',
      });

      expect(config).toEqual({
        content: 'Some important text content',
        label: 'My notes',
      });
    });

    it('generates default name from label', () => {
      const name = text.getDefaultName({
        textLabel: 'My notes',
      });

      expect(name).toBe('My notes');
    });

    it('uses fallback name when no label', () => {
      const name = text.getDefaultName({
        textLabel: '',
      });

      expect(name).toBe('Text content');
    });
  });

  describe('Discord Source', () => {
    const discord = getSourceById('discord')!;

    it('is disabled', () => {
      expect(discord.enabled).toBe(false);
    });

    it('builds empty config', () => {
      const config = discord.buildConfig({});

      expect(config).toEqual({
        server_id: '',
      });
    });

    it('returns default name', () => {
      const name = discord.getDefaultName({});
      expect(name).toBe('Discord');
    });

    it('returns empty field IDs', () => {
      const ids = discord.getFieldIds();
      expect(ids).toEqual([]);
    });
  });

  describe('Slack Source', () => {
    const slack = getSourceById('slack')!;

    it('is disabled', () => {
      expect(slack.enabled).toBe(false);
    });

    it('builds empty config', () => {
      const config = slack.buildConfig({});

      expect(config).toEqual({
        workspace_id: '',
      });
    });

    it('returns default name', () => {
      const name = slack.getDefaultName({});
      expect(name).toBe('Slack');
    });

    it('returns empty field IDs', () => {
      const ids = slack.getFieldIds();
      expect(ids).toEqual([]);
    });
  });

  describe('iCal Source', () => {
    const ical = getSourceById('ical')!;

    it('builds config correctly', () => {
      const config = ical.buildConfig({
        icalUrl: 'https://calendar.example.com/feed.ics',
      });

      expect(config).toEqual({
        url: 'https://calendar.example.com/feed.ics',
      });
    });

    it('generates default name from URL hostname', () => {
      const name = ical.getDefaultName({
        icalUrl: 'https://calendar.google.com/feed.ics',
      });

      expect(name).toBe('calendar.google.com');
    });

    it('returns fallback name for invalid URL', () => {
      const name = ical.getDefaultName({
        icalUrl: 'not-a-valid-url',
      });

      expect(name).toBe('Calendar');
    });

    it('returns field IDs', () => {
      const ids = ical.getFieldIds();
      expect(ids).toContain('icalUrl');
    });
  });
});

describe('Source Edge Cases', () => {
  describe('GitLab Source Defaults', () => {
    const gitlab = getSourceById('gitlab')!;

    it('uses default host when empty', () => {
      const config = gitlab.buildConfig({
        glProjectId: 'mygroup/myproject',
        glHost: '',
        glBranch: 'main',
      });

      expect(config).toEqual({
        project_id: 'mygroup/myproject',
        host: 'https://gitlab.com',
        branch: 'main',
      });
    });

    it('uses default branch when empty', () => {
      const config = gitlab.buildConfig({
        glProjectId: 'mygroup/myproject',
        glHost: 'https://gitlab.example.com',
        glBranch: '',
      });

      expect(config).toEqual({
        project_id: 'mygroup/myproject',
        host: 'https://gitlab.example.com',
        branch: 'main',
      });
    });

    it('returns field IDs', () => {
      const ids = gitlab.getFieldIds();
      expect(ids).toContain('glHost');
      expect(ids).toContain('glProjectId');
      expect(ids).toContain('glBranch');
    });
  });

  describe('Filesystem Source Defaults', () => {
    const fs = getSourceById('filesystem')!;

    it('returns path as name when no folder found', () => {
      // Test edge case where path.split('/').pop() might return empty
      const name = fs.getDefaultName({
        fsBasePath: '',
      });

      expect(name).toBe('');
    });

    it('returns field IDs', () => {
      const ids = fs.getFieldIds();
      expect(ids).toContain('fsBasePath');
      expect(ids).toContain('fsAllowWrites');
    });
  });

  describe('IMAP Source Defaults', () => {
    const imap = getSourceById('imap')!;

    it('uses INBOX as default folder when empty', () => {
      const config = imap.buildConfig({
        imapHost: 'imap.example.com',
        imapPort: 993,
        imapUsername: 'user@example.com',
        imapUseSsl: true,
        imapFolder: '',
      });

      expect(config).toEqual({
        host: 'imap.example.com',
        port: 993,
        username: 'user@example.com',
        use_ssl: true,
        folder: 'INBOX',
      });
    });

    it('returns field IDs', () => {
      const ids = imap.getFieldIds();
      expect(ids).toContain('imapHost');
      expect(ids).toContain('imapPort');
      expect(ids).toContain('imapUsername');
      expect(ids).toContain('imapUseSsl');
      expect(ids).toContain('imapFolder');
    });
  });

  describe('Text Source Defaults', () => {
    const text = getSourceById('text')!;

    it('builds config with undefined label when empty', () => {
      const config = text.buildConfig({
        textContent: 'Some content',
        textLabel: '',
      });

      expect(config).toEqual({
        content: 'Some content',
        label: undefined,
      });
    });

    it('returns field IDs', () => {
      const ids = text.getFieldIds();
      expect(ids).toContain('textLabel');
      expect(ids).toContain('textContent');
    });
  });

  describe('Web Source', () => {
    const web = getSourceById('web')!;

    it('returns field IDs', () => {
      const ids = web.getFieldIds();
      expect(ids).toContain('webUrl');
    });
  });
});
