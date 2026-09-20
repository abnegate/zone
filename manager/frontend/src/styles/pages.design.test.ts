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

  it('ellipsises the title before the hover actions on a matching background', () => {
    const fade = rule(chats, '.chat-item-actions::before');
    expect(fade).toContain('right: 100%');
    expect(fade).toContain('width: var(--ui-space-2)');
    expect(fade).toContain('linear-gradient(to right, transparent, var(--chat-item-bg))');
    expect(rule(chats, '.chat-item-actions')).toContain('background: var(--chat-item-bg)');
    expect(rule(chats, '.chat-item:hover,\n.chat-item:focus-within')).toContain(
      '--chat-item-bg: var(--ui-bg-hover)'
    );
    expect(rule(chats, '.chat-item.active')).toContain('--chat-item-bg: var(--ui-accent-muted)');
    expect(
      rule(
        chats,
        '.chat-item:hover .chat-item-content,\n.chat-item:focus-within .chat-item-content'
      )
    ).toContain('padding-right: calc(var(--chat-item-actions) + var(--ui-space-2))');
    expect(rule(chats, '.chat-title')).toContain('text-overflow: ellipsis');
    expect(chats).not.toContain('text-overflow: clip');
  });

  it('sets a chat item to 52px: 8 + 18 + 2 + 16 + 8', () => {
    expect(rule(chats, '.chat-item')).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
    expect(rule(chats, '.chat-title')).toContain('line-height: 1.125rem');
    expect(rule(chats, '.chat-meta')).toContain('margin-top: var(--ui-space-0-5)');
    expect(rule(chats, '.chat-meta')).toContain('line-height: var(--ui-space-4)');
  });

  it('sets a search result to the 56px two-line row: 4 + 16 title + 32 snippet + 4', () => {
    expect(rule(chats, '.search-result-item')).toContain('height: var(--ui-list-row-2)');
    expect(rule(chats, '.search-result-item')).toContain(
      'padding: var(--ui-space-1) var(--ui-space-3)'
    );
    expect(rule(chats, '.search-result-header')).toContain('height: var(--ui-space-4)');
    expect(rule(chats, '.search-result-chat')).toContain('flex: 1');
    expect(rule(chats, '.search-result-chat')).toContain('line-height: var(--ui-space-4)');
    expect(rule(chats, '.search-result-date')).toContain('line-height: var(--ui-space-4)');
    expect(rule(chats, '.search-result-snippet')).toContain('-webkit-line-clamp: 2');
    expect(rule(chats, '.search-result-snippet')).toContain('height: var(--ui-space-8)');
    expect(rule(chats, '.search-result-snippet')).toContain('line-height: var(--ui-space-4)');
    expect(chats).not.toContain('.search-result-score');
  });

  it('positions the message column so hidden text cannot stretch the document', () => {
    expect(rule(chats, '.messages-container')).toContain('position: relative');
    expect(rule(chats, '.message-markdown')).toContain('position: relative');
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

  it('lays the composer out as one row of 28px controls around a 24px draft line', () => {
    expect(chats).not.toContain('.message-form-tools');
    expect(rule(chats, '.message-form')).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
    expect(rule(chats, '.message-form-row')).toContain('align-items: flex-end');
    const draft = rule(chats, '.message-form textarea');
    expect(draft).toContain('flex: 1 1 8rem');
    expect(draft).toContain('min-height: var(--ui-control-height-sm)');
    expect(draft).toContain('max-height: 12.5rem');
    expect(draft).toContain('line-height: 1.5rem');
    expect(rule(chats, '.chat-sources-toggle,\n.chat-source-chip')).toContain(
      'height: var(--ui-control-height-sm)'
    );
  });

  it('labels the reasoning of a turn once, on one activity block with one left rule', () => {
    expect(chats).not.toContain('.message-reasoning summary');
    expect(rule(chats, '.message-activity')).toContain('border-left: 2px solid var(--ui-border)');
    expect(rule(chats, '.message-activity-toggle')).toContain('height: var(--ui-space-5)');
    expect(rule(chats, '.message-activity-toggle')).toContain('text-transform: uppercase');
    expect(rule(chats, '.message-reasoning')).not.toContain('border-left');
    expect(rule(chats, '.message-reasoning')).toContain('line-height: var(--ui-space-5)');
    expect(rule(chats, '.tool-trace')).not.toContain('border-left');
  });
});

describe('context meter', () => {
  const meter = read(join(features, 'chats', 'components', 'ContextUsage.css'));

  it('is a 20px row whose details open as a popover', () => {
    expect(rule(meter, '.context-usage-toggle')).toContain('height: var(--ui-space-5)');
    expect(rule(meter, '.context-usage-details')).toContain('position: absolute');
    expect(meter).not.toContain('--text-secondary');
  });

  it('discloses with a 12px chevron that turns when open', () => {
    expect(rule(meter, '.context-usage-caret')).toContain('width: var(--ui-space-3)');
    expect(rule(meter, '.context-usage-caret')).toContain('height: var(--ui-space-3)');
    expect(rule(meter, ".context-usage-caret[data-expanded='true']")).toContain('rotate(180deg)');
  });
});

describe('question card and receipts', () => {
  it('lays each choice on one 32px row with the free-text box on its own row below', () => {
    const question = read(join(features, 'chats', 'components', 'QuestionCard.css'));
    expect(rule(question, '.question-card-choice-row')).toContain(
      'height: var(--ui-control-height)'
    );
    expect(rule(question, '.question-card-choice')).toContain('gap: var(--ui-space-2)');
    expect(rule(question, '.question-card-choices')).toContain('gap: var(--ui-space-1)');
    expect(rule(question, '.question-card-text')).toContain('height: var(--ui-control-height)');
    expect(rule(question, '.question-card-text')).not.toContain('flex-basis');
    expect(rule(question, '.question-card-text')).toContain(
      'width: calc(100% - var(--ui-space-8) - var(--ui-space-2))'
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

  it('centres the search empty state in the body like the other list pages', () => {
    expect(rule(search, '.context-search-body')).toContain('flex-direction: column');
    expect(rule(search, '.context-search-content')).toContain('flex-direction: column');
    expect(rule(search, '.context-search-content > .ui-empty')).toContain('margin: auto');
  });

  it('mutes the wiki card excerpt slot when an entry has nothing to show there', () => {
    expect(rule(wiki, '.knowledge-card-content--empty')).toContain('color: var(--ui-text-muted)');
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
    const title = rule(auth, '.auth-error-state .error-title,\n.auth-success .success-message');
    expect(title).toContain('font-size: var(--ui-text-md)');
    expect(title).toContain('font-family: var(--ui-font-body)');
  });

  it('leaves the invitation page no error card of its own', () => {
    expect(invitation).not.toContain('.error-state');
    expect(invitation).not.toContain('.loading-state');
  });

  it('sizes session rows at 44 with a 28px pager', () => {
    const sessions = read(join(features, 'auth', 'pages', 'SessionsPage.css'));
    expect(rule(sessions, '.sessions-table td')).toContain('height: 2.75rem');
    expect(rule(sessions, '.sessions-table td')).toContain('padding: 0 var(--ui-space-3)');
    expect(rule(sessions, '.sessions-table td')).toContain('box-shadow: inset 0 -1px');
    expect(rule(sessions, '.sessions-pager')).toContain('height: var(--ui-control-height-sm)');
    expect(rule(sessions, '.current-badge')).toContain('height: var(--ui-badge-height)');
  });
});

const legacyScales = /var\(--(gray|blue|green|red|yellow|purple)-\d+\)/;

describe('tasks page layout', () => {
  const css = read(join(features, 'tasks', 'pages', 'TasksPage.css'));

  it('gives every card the same slots so a grid row shares one height', () => {
    expect(rule(css, '.tasks-list')).not.toContain('grid-auto-rows');
    expect(rule(css, '.task-card-title')).toContain('height: var(--ui-space-5)');
    expect(rule(css, '.task-project')).toContain('height: var(--ui-space-4)');
    expect(rule(css, '.task-card .task-description')).toContain('height: var(--ui-space-10)');
    expect(rule(css, '.task-card .task-meta')).toContain('height: var(--ui-badge-height)');
    expect(rule(css, '.task-actions')).toContain('margin-top: auto');
  });

  it('guarantees the title 60% of its row and keeps the badges from pushing it out', () => {
    expect(rule(css, '.task-card-title h3')).toContain('min-width: 60%');
    expect(rule(css, '.task-badges')).toContain('overflow: hidden');
    expect(rule(css, '.task-badges')).not.toContain('flex-shrink: 0');
  });

  it('keeps badges and pull request rows on the palette', () => {
    expect(css).not.toMatch(legacyScales);
    expect(css).not.toContain('.task-agentic-badge');
    expect(css).not.toContain('.task-pr-info');
    expect(rule(css, '.task-branch')).toContain('height: var(--ui-badge-height)');
  });

  it('folds the pull request into the meta row and lets only the branch tag give way', () => {
    expect(rule(css, '.task-pr')).toContain('display: contents');
    expect(rule(css, '.task-pr::before')).toContain('margin-right: 0');
    expect(css).toContain('.task-meta > * + *::before');
    const branch = rule(css, '.task-branch');
    expect(branch).toContain('flex: 0 1 auto');
    expect(branch).toContain('min-width: 0');
    expect(branch).toContain('max-width: 50%');
    expect(branch).toContain('text-overflow: ellipsis');
  });

  it('lets a wizard toggle keep its description on the line under the title', () => {
    expect(css).not.toContain('.task-wizard .toggle-text');
    expect(css).not.toContain('.task-wizard .toggle-desc');
    expect(css).not.toContain('.task-wizard .toggle-label');
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

  it('makes every list card exactly 72px on 8/12 padding', () => {
    const card = rule(css, '.project-card.card--list');
    expect(card).toContain('height: 4.5rem');
    expect(card).toContain('box-sizing: border-box');
    expect(card).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
    expect(card).not.toContain('gap:');
    expect(rule(css, '.project-card-header')).toContain('height: var(--ui-space-5)');
    expect(rule(css, '.project-description')).toContain('line-height: 1.125rem');
    expect(rule(css, '.project-card-footer')).toContain('height: var(--ui-space-4)');
  });

  it('stacks a wizard source tile as a 56px name-over-description row so nothing truncates', () => {
    expect(rule(css, '.source-selection-option')).toContain('min-height: var(--ui-list-row-2)');
    expect(rule(css, '.source-selection-info')).toContain('flex-direction: column');
    expect(rule(css, '.source-selection-name')).toContain('line-height: var(--ui-space-5)');
    expect(rule(css, '.source-selection-desc')).toContain('line-height: var(--ui-space-4)');
    expect(css).toContain('.status-selection-option {\n  min-height: var(--ui-space-12);');
  });

  it('sets the detail title in the body face', () => {
    const title = rule(css, '.details-header h2');
    expect(title).toContain('font-family: var(--ui-font-body)');
    expect(title).toContain('font-size: var(--ui-heading-size)');
    expect(title).not.toContain('display');
  });

  it('lays the detail pane out as a 48px header, a facts grid and a 48px footer', () => {
    expect(rule(css, '.details-header')).toContain('height: var(--ui-header-height)');
    expect(rule(css, '.detail-facts')).toContain('grid-template-columns: 6rem minmax(0, 1fr)');
    expect(rule(css, '.details-actions')).toContain('height: var(--ui-header-height)');
    expect(rule(css, '.details-actions')).toContain('justify-content: flex-end');
  });
});

describe('sources page layout', () => {
  const css = read(join(features, 'sources', 'pages', 'SourcesPage.css'));

  it('makes every source card exactly 104px on 8/12 padding', () => {
    const card = rule(css, '.source-card.card--list');
    expect(card).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
    expect(card).not.toContain('gap:');
    expect(rule(css, '.source-card-title')).toContain('height: var(--ui-badge-height)');
    expect(rule(css, '.source-description')).toContain('height: 1.125rem');
    expect(rule(css, '.source-description')).toContain('line-height: 1.125rem');
    expect(rule(css, '.source-url')).toContain('line-height: var(--ui-space-4)');
    expect(rule(css, '.source-card-meta')).toContain('height: var(--ui-control-height-sm)');
    expect(rule(css, '.source-card-meta')).toContain('margin-top: var(--ui-space-1)');
  });
});

describe('settings page layout', () => {
  const css = read(join(features, 'settings', 'workspace', 'pages', 'WorkspaceSettingsPage.css'));

  it('sticks the save footer to the scroller edge so it does not lift at the end of the scroll', () => {
    const footer = rule(css, '.settings-actions');
    expect(footer).toContain('position: sticky');
    expect(footer).toContain('bottom: 0;');
    expect(footer).not.toContain('--ui-panel-padding');
    expect(footer).toContain('margin: auto calc(-1 * var(--ui-space-1)) 0');
    const body = rule(css, '.settings-page .page-body:has(.settings-actions)');
    expect(body).toContain('padding-bottom: 0');
    expect(body).toContain('flex-direction: column');
  });
});

describe('models page layout', () => {
  const css = read(join(features, 'models', 'pages', 'ModelsPage.css'));
  const page = read(join(features, 'models', 'pages', 'ModelsPage.tsx'));
  const modals = read(join(import.meta.dir, 'modals.css'));

  it('adds a model from one 32px row with the hint under it', () => {
    expect(page).not.toContain('<h2>Add Model</h2>');
    expect(rule(css, '.models-install-form')).toContain('height: var(--ui-control-height)');
    const hint = rule(css, '.models-install-panel .help-text');
    expect(hint).toContain('line-height: var(--ui-space-4)');
    expect(hint).toContain('font-size: var(--ui-text-2xs)');
  });

  it('shows capabilities and raw tags as one row', () => {
    expect(page).not.toContain('details-tags');
    expect(modals).not.toContain('.details-tags');
  });
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
