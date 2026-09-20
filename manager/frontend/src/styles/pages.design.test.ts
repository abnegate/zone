import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const features = join(import.meta.dir, '..', 'features');

function read(path: string): string {
  return readFileSync(path, 'utf8');
}

function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`(^|\\n)${escaped}\\s*\\{([^}]*)\\}`));
  if (!match) throw new Error(`rule ${selector} is not defined`);
  return match[2];
}

describe('chats layout', () => {
  const chats = read(join(features, 'chats', 'pages', 'ChatsPage.css'));

  it('keeps the hover actions out of the chat title width', () => {
    const actions = rule(chats, '.chat-item-actions');
    expect(actions).toContain('position: absolute');
    expect(actions).toContain('display: none');
    expect(
      rule(
        chats,
        '.chat-item:hover .chat-item-actions,\n.chat-item:focus-within .chat-item-actions'
      )
    ).toContain('display: flex');
  });

  it('holds the conversation header to one 48px row', () => {
    const header = rule(chats, '.chat-header');
    expect(header).toContain('flex-wrap: nowrap');
    expect(header).toContain('height: var(--ui-header-height)');
    expect(rule(chats, '.chat-header-info h3')).toContain('white-space: nowrap');
  });

  it('draws the reader bubble at most 72% wide with the time outside it', () => {
    expect(rule(chats, '.message-user')).toContain('max-width: 72%');
    expect(rule(chats, '.message-user .message-content')).toContain('background: var(--ui-accent)');
    expect(rule(chats, '.message-user .message-header')).toContain('order: 2');
  });

  it('sets chat prose at 14/22 and code at 12', () => {
    expect(rule(chats, '.message')).toContain('font-size: var(--ui-text-md)');
    expect(rule(chats, '.message')).toContain('line-height: 1.375rem');
    expect(rule(chats, '.message-markdown pre')).toContain('font-size: var(--ui-text-xs)');
  });

  it('bounds media to 480 and the composer to the reading column', () => {
    expect(rule(chats, '.message-images,\n.message-videos,\n.message-audios')).toContain('30rem');
    expect(rule(chats, '.chat-composer')).toContain('max-width: var(--chat-column)');
  });
});

describe('context meter', () => {
  const meter = read(join(features, 'chats', 'components', 'ContextUsage.css'));

  it('is a 20px row whose details open as a popover', () => {
    expect(rule(meter, '.context-usage-toggle')).toContain('height: var(--ui-space-5)');
    expect(rule(meter, '.context-usage-details')).toContain('position: absolute');
    expect(meter).not.toContain('--text-secondary');
  });
});

describe('question card and receipts', () => {
  it('lays each choice on one 32px row', () => {
    const question = read(join(features, 'chats', 'components', 'QuestionCard.css'));
    expect(rule(question, '.question-card-choice')).toContain(
      'min-height: var(--ui-control-height)'
    );
    expect(rule(question, '.question-card-choice-description::before')).toContain("content: '— '");
  });

  it('keeps a receipt to a title row, a summary and one meta row', () => {
    const receipts = read(join(features, 'chats', 'components', 'ActionReceipts.css'));
    expect(rule(receipts, '.action-receipt')).toContain('padding: var(--ui-space-3)');
    expect(rule(receipts, '.action-receipt-meta')).toContain('display: flex');
    expect(receipts).not.toContain('grid-template-columns: repeat(auto-fit');
  });
});

describe('knowledge layout', () => {
  const wiki = read(join(features, 'knowledge', 'pages', 'WikiPage.css'));
  const search = read(join(features, 'knowledge', 'pages', 'ContextSearchPage.css'));

  it('gives every wiki card the same 96px height', () => {
    expect(rule(wiki, '.knowledge-card')).toContain('height: 6rem');
    expect(rule(wiki, '.knowledge-grid')).toContain('grid-auto-rows: 6rem');
    expect(rule(wiki, '.knowledge-card')).not.toContain('translateY');
  });

  it('leaves the frame to scroll the wiki body', () => {
    expect(wiki).not.toContain('.wiki-workspace');
    expect(wiki).not.toContain('radial-gradient');
  });

  it('puts the search controls on one toolbar row without a card or a score bar', () => {
    expect(search).not.toContain('.search-section');
    expect(search).not.toContain('.relevance-bar');
    expect(rule(search, '.search-toolbar')).toContain('display: flex');
    expect(rule(search, '.source-pill')).toContain('height: var(--ui-control-height-sm)');
  });
});

describe('auth layout', () => {
  const auth = read(join(features, 'auth', 'pages', 'AuthPage.css'));
  const invitation = read(join(features, 'auth', 'pages', 'InvitationAcceptPage.css'));

  it('uses one 400px card with a flat title', () => {
    expect(rule(auth, '.auth-container')).toContain('max-width: 25rem');
    expect(rule(invitation, '.invitation-card')).toContain('max-width: 25rem');
    expect(auth).not.toContain('linear-gradient');
    expect(auth).not.toContain('--color-bg');
  });

  it('sizes the status states like the empty state', () => {
    expect(rule(auth, '.auth-success .success-icon,\n.auth-error-state .error-icon')).toContain(
      'width: var(--ui-empty-icon-size)'
    );
    expect(rule(auth, '.auth-error-state .error-title,\n.auth-success .success-message')).toContain(
      'font-size: var(--ui-text-md)'
    );
  });
});

const legacyScales = /var\(--(gray|blue|green|red|yellow|purple)-\d+\)/;

describe('tasks page layout', () => {
  const css = read(join(features, 'tasks', 'pages', 'TasksPage.css'));

  it('gives every card the same slots so a grid row shares one height', () => {
    expect(rule(css, '.tasks-list')).toContain('grid-auto-rows: 1fr');
    expect(rule(css, '.task-card-title')).toContain('height: var(--ui-space-5)');
    expect(rule(css, '.task-card .task-description')).toContain('height: var(--ui-space-10)');
    expect(rule(css, '.task-card .task-meta')).toContain('height: var(--ui-badge-height)');
    expect(rule(css, '.task-actions')).toContain('margin-top: auto');
  });

  it('keeps badges and pull request rows on the palette', () => {
    expect(css).not.toMatch(legacyScales);
    expect(css).not.toContain('.task-agentic-badge');
    expect(css).not.toContain('.task-pr-info');
    expect(rule(css, '.task-branch')).toContain('height: var(--ui-badge-height)');
  });

  it('shows the execution log as bounded rows', () => {
    expect(rule(css, '.logs-container')).toContain('max-height: 40vh');
    expect(rule(css, '.log-entry')).toContain('min-height: var(--ui-control-height-sm)');
  });
});

describe('projects page layout', () => {
  const css = read(join(features, 'projects', 'pages', 'ProjectsPage.css'));

  it('draws no gradient behind the page and keeps the list pane at 320', () => {
    expect(css).not.toContain('radial-gradient');
    expect(rule(css, '.projects-list-pane')).toContain('width: 20rem');
  });

  it('lays the detail pane out as a 48px header, a facts grid and a 48px footer', () => {
    expect(rule(css, '.details-header')).toContain('height: var(--ui-header-height)');
    expect(rule(css, '.detail-facts')).toContain('grid-template-columns: 6rem minmax(0, 1fr)');
    expect(rule(css, '.details-actions')).toContain('height: var(--ui-header-height)');
    expect(rule(css, '.details-actions')).toContain('justify-content: flex-end');
  });
});

describe('models page layout', () => {
  const css = read(join(features, 'models', 'pages', 'ModelsPage.css'));
  const browse = read(join(features, 'models', 'components', 'VirtualBrowseList.tsx'));
  const pulls = read(join(features, 'models', 'components', 'PullJobs.css'));

  it('lists installed models and the catalog without content cards', () => {
    expect(css).not.toContain('.card.models-list-panel');
    expect(css).not.toContain('.card.models-browse-panel');
    expect(rule(css, '.models-section-head')).toContain('height: var(--ui-control-height-sm)');
    expect(rule(css, '.browse-filters .filter-pill')).toContain('height: var(--ui-space-6)');
  });

  it('gives the virtual catalog fixed two-line rows', () => {
    expect(browse).toContain('const ROW_HEIGHT = 72;');
    expect(browse).toContain('rowHeight={ROW_HEIGHT}');
    expect(browse).not.toContain('useDynamicRowHeight');
  });

  it('keeps a pull job to a 40px row with a 4px bar', () => {
    expect(rule(pulls, '.pull-job')).toContain('min-height: var(--ui-list-row)');
    expect(rule(pulls, '.pull-job-row')).toContain('height: var(--ui-space-5)');
  });
});
