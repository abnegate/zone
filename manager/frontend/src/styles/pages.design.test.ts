import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule, token } from '../test/css';

const features = join(import.meta.dir, '..', 'features');

describe('chats layout', () => {
  const chats = read(join(features, 'chats', 'pages', 'ChatsPage.css'));

  it('keeps the hover actions out of the chat title width', () => {
    const actions = rule(chats, '.chat-item-actions');
    expect(actions).toContain('position: absolute');
    expect(actions).toContain('opacity: 0');
    expect(
      rule(
        chats,
        ".chat-item:hover .chat-item-actions,\n.chat-item:focus-within .chat-item-actions,\n.chat-item-actions:has(.ui-action-menu-trigger[data-state='open'])"
      )
    ).toContain('opacity: 1');
  });

  it('hides the row menu trigger without removing it, so focus can return to it', () => {
    expect(rule(chats, '.chat-item-actions')).not.toContain('display: none');
    expect(rule(chats, '.chat-item-actions')).not.toContain('visibility');
  });

  it('keeps the row menu trigger shown and its row highlighted while the menu is open', () => {
    expect(
      rule(
        chats,
        ".chat-item:hover,\n.chat-item:focus-within,\n.chat-item:has(.ui-action-menu-trigger[data-state='open'])"
      )
    ).toContain('--chat-item-bg: var(--ui-bg-hover)');
    expect(rule(chats, '.chat-item')).toContain('--chat-item-actions: var(--ui-control-height-sm)');
    expect(media(chats, '(hover: none)')).toContain('opacity: 1');
  });

  it('ellipsises the title before the hover actions on a matching background', () => {
    const fade = rule(chats, '.chat-item-actions::before');
    expect(fade).toContain('right: 100%');
    expect(fade).toContain('width: var(--ui-space-2)');
    expect(fade).toContain('linear-gradient(to right, transparent, var(--chat-item-bg))');
    expect(rule(chats, '.chat-item-actions')).toContain('background: var(--chat-item-bg)');
    expect(rule(chats, '.chat-item.active')).toContain('--chat-item-bg: var(--ui-bg-selected)');
    expect(chats).not.toMatch(/\.chat-item:(hover|focus-within) \.chat-item-content/);
    expect(rule(chats, '.chat-title')).toContain('text-overflow: ellipsis');
    expect(chats).not.toContain('text-overflow: clip');
  });

  it('covers the whole row with the hover actions, on a background nothing shows through', () => {
    const actions = rule(chats, '.chat-item-actions');
    expect(actions).toContain('top: 0');
    expect(actions).toContain('bottom: 0');
    expect(actions).toContain('align-items: center');
    expect(actions).not.toContain('translateY');
    const fade = rule(chats, '.chat-item-actions::before');
    expect(fade).toContain('top: 0');
    expect(fade).toContain('bottom: 0');

    const variables = read(
      join(
        import.meta.dir,
        '..',
        '..',
        '..',
        '..',
        'packages',
        'ui',
        'src',
        'styles',
        'variables.css'
      )
    );
    const selected = variables.match(/--ui-bg-selected:[^;]*;/g) ?? [];
    expect(selected.length).toBe((variables.match(/--ui-accent-muted:/g) ?? []).length);
    for (const value of selected) expect(value).not.toContain('transparent');
  });

  it('sets a chat item to 52px: 8 + 18 + 2 + 16 + 8', () => {
    expect(rule(chats, '.chat-item')).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
    expect(rule(chats, '.chat-title')).toContain('line-height: 1.125rem');
    expect(rule(chats, '.chat-meta')).toContain('margin-top: var(--ui-space-0-5)');
    expect(rule(chats, '.chat-meta')).toContain('line-height: var(--ui-space-4)');
  });

  it('sticks group headers inside the list scroller at 28px', () => {
    const header = rule(chats, '.chat-group-header');
    expect(header).toContain('position: sticky');
    expect(header).toContain('top: 0');
    expect(header).toContain('height: var(--ui-control-height-sm)');
    expect(header).toContain('background: var(--ui-bg-base)');
    expect(rule(chats, '.chats-list')).toContain('overflow-y: auto');
    expect(rule(chats, '.chat-group')).not.toContain('overflow');
  });

  it('uses the shared select trigger for every chat header select', () => {
    const trigger = rule(
      chats,
      '.chat-provider.ui-select-trigger, .chat-provider-model.ui-select-trigger, .reasoning-effort.ui-select-trigger, .context-tokens.ui-select-trigger'
    );
    expect(trigger).toContain('height: var(--ui-control-height-sm)');
    expect(trigger).toContain('border-radius: var(--ui-radius-full)');
    expect(chats).not.toContain('.reasoning-effort select');
    expect(chats).not.toContain('.context-tokens select');
  });

  it('grows the chat header to fit a provider error under its controls', () => {
    expect(chats).not.toContain('.chat-header:has(.chat-provider-error)');
    const error = rule(chats, '.chat-provider-error');
    expect(error).toContain('order: 1');
    expect(error).toContain('flex: 1 1 100%');
    expect(error).toContain('color: var(--ui-error)');
  });

  it('leaves chat list width to SubNav', () => {
    expect(rule(chats, '.chats-sidebar')).not.toContain('width:');
    expect(rule(chats, '.chats-sidebar')).not.toContain('min-width:');
  });

  it('keeps group and sort controls out of the 48px page-bar', () => {
    expect(rule(chats, '.chats-sidebar-header.page-bar')).not.toContain('chats-arrange');
    expect(rule(chats, '.chats-arrange')).toContain('flex-shrink: 0');
    expect(rule(chats, '.chats-arrange .ui-select-trigger')).toContain(
      'height: var(--ui-control-height-sm)'
    );
    expect(rule(chats, '.chats-arrange .ui-select-trigger')).toContain(
      'font-size: var(--ui-text-xs)'
    );
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

  it('holds the conversation header to one 48px row until its controls would crowd out the title', () => {
    const header = rule(chats, '.chat-header');
    expect(header).toContain('flex-wrap: wrap');
    expect(header).toContain('min-height: var(--ui-header-height)');
    expect(header).not.toMatch(/(^|[^-])height: var\(--ui-header-height\)/m);
    expect(rule(chats, '.chat-header-info')).toContain('flex: 1 1 14rem');
    const actions = rule(chats, '.chat-header-actions');
    expect(actions).toContain('flex-wrap: wrap');
    expect(actions).toContain('justify-content: flex-end');
    expect(rule(chats, '.chat-header-info h3')).toContain('white-space: nowrap');
  });

  it('wraps the conversation header under the title on a phone', () => {
    const phone = media(chats, '(max-width: 768px)');
    expect(rule(phone, '.chat-header')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.chat-header')).toContain('height: auto');
    expect(rule(phone, '.chat-back')).toContain('display: inline-flex');
    expect(rule(phone, '.chat-header-info')).toContain('flex: 1 1 100%');
    expect(rule(phone, '.chat-header-info')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.chat-header-actions')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.chat-header-actions')).toContain('flex: 1 1 100%');
    expect(rule(phone, '.chat-header-actions')).toContain('justify-content: flex-start');
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

  it('puts attached sources under the composer, not in the draft row', () => {
    expect(chats).not.toContain('.message-form-row .chat-sources-bar');
    expect(rule(chats, '.chat-composer-footer')).toContain('display: flex');
    expect(rule(chats, '.chat-composer-footer .chat-sources-bar')).toContain('flex: 1 1 auto');
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
    expect(wiki).not.toContain('.search-section');
    expect(wiki).not.toContain('.relevance-bar');
    expect(rule(wiki, '.search-toolbar')).toContain('display: flex');
    expect(rule(wiki, '.source-pill')).toContain('height: var(--ui-control-height-sm)');
  });

  it('sizes the wiki search field as a flex form in the page bar', () => {
    expect(rule(wiki, '.wiki-search.search-form')).toContain('display: flex');
    expect(rule(wiki, '.wiki-search.search-form')).toContain('max-width: 20rem');
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

  it('sizes session rows at 40 with a 28px pager', () => {
    const sessions = read(join(features, 'auth', 'components', 'SessionsSection.css'));
    expect(rule(sessions, '.sessions-table td')).toContain('height: var(--ui-list-row)');
    expect(rule(sessions, '.sessions-table td')).toContain('padding: 0 var(--ui-space-3)');
    expect(rule(sessions, '.sessions-table td')).toContain('box-shadow: inset 0 -1px');
    expect(rule(sessions, '.sessions-pager')).toContain('height: var(--ui-control-height-sm)');
    expect(rule(sessions, '.current-badge')).toContain('height: var(--ui-badge-height)');
  });
});

const legacyScales = /var\(--(gray|blue|green|red|yellow|purple)-\d+\)/;

describe('tasks page layout', () => {
  const css = read(join(features, 'tasks', 'pages', 'TasksPage.css'));
  const boardStyles = join(features, 'tasks', 'components', 'board');
  const board = read(join(boardStyles, 'Board.css'));
  const columns = read(join(boardStyles, 'Column.css'));
  const cards = read(join(boardStyles, 'Card.css'));
  const SELECTING_SELECTOR =
    '.board[data-selecting] .task-card-selector, .task-card[data-selected] .task-card-selector';
  const SELECTING_CARD = '.board[data-selecting] .task-card, .task-card[data-selected]';
  const SELECTING_GRIP =
    '.board[data-selecting] .task-card-handle::after, .task-card[data-selected] .task-card-handle::after';

  it('scrolls the board sideways in its own row of fixed-width columns', () => {
    expect(rule(board, '.board')).toContain('overflow-x: auto');
    const column = rule(columns, '.board-column');
    expect(column).toContain('flex-shrink: 0');
    expect(column).toContain('width: 280px');
  });

  it('keeps every column rule in the column stylesheet', () => {
    expect(board).not.toMatch(/(^|[\s,}])\.board-column\s*[{,]/);
  });

  it('gives the drag grip a 24px hit area, and 44px on a touch screen, without growing the grip', () => {
    const handle = rule(cards, '.task-card-handle');
    expect(handle).toContain('width: var(--ui-space-3)');
    expect(handle).toContain('height: var(--ui-space-5)');
    expect(handle).toContain('--task-card-handle-target: var(--ui-space-6)');
    const grip = rule(cards, '.task-card-handle svg');
    expect(grip).toContain('width: 0.5rem');
    expect(grip).toContain('height: var(--ui-space-3)');

    const target = rule(cards, '.task-card-handle::after');
    expect(target).toContain('position: absolute');
    expect(target).toContain('width: var(--task-card-handle-target)');
    expect(target).toContain('height: var(--task-card-handle-target)');
    expect(target).toContain("content: ''");

    const touch = media(cards, '(any-pointer: coarse)');
    expect(rule(touch, '.task-card-handle')).toContain('--task-card-handle-target: 2.75rem');
    expect(rule(touch, '.task-card-handle::after')).toContain('transform: none');
  });

  it('ends the grip hit area at the grip, clear of the title beside it', () => {
    const target = rule(cards, '.task-card-handle::after');
    expect(target).toContain('right: 0');
    expect(target).toContain('transform: translateY(-50%)');
    expect(target).not.toContain('left:');
  });

  it('shows the grip and the menu wherever any pointer is coarse, not only without hover', () => {
    expect(cards).not.toContain('(hover: none)');
    const touch = media(cards, '(any-pointer: coarse)');
    expect(rule(touch, '.task-card-handle, .task-card-menu')).toContain('opacity: 1');
  });

  it('shows the card checkboxes only while selecting, never on hover or focus', () => {
    const selector = rule(cards, '.task-card-selector');
    expect(selector).toContain('display: none');
    expect(selector).not.toContain('opacity');
    expect(rule(cards, SELECTING_SELECTOR)).toBe('display: flex;');
    expect(cards).not.toMatch(/:(hover|focus-within) \.task-card-selector/);
    expect(media(cards, '(any-pointer: coarse)')).not.toContain('.task-card-selector');
  });

  it('indents the title and everything under it together, and only while selecting', () => {
    expect(rule(cards, '.task-card-heading')).not.toContain('padding-left');
    expect(rule(cards, SELECTING_CARD)).toBe(
      'padding-left: calc( var(--task-card-select-left) + var(--task-card-select-size) + var(--task-card-select-gap) );'
    );
  });

  it('gives the grip its whole 44px square on a touch screen until selecting shares it with the checkbox', () => {
    const touch = media(cards, '(any-pointer: coarse)');
    expect(rule(touch, '.task-card-handle::after')).not.toContain('width');
    expect(rule(touch, SELECTING_GRIP)).toBe(
      'width: calc(var(--task-card-select-left) + var(--ui-space-2));'
    );
    const card = rule(touch, '.task-card');
    expect(card).toContain('--task-card-select-above: var(--ui-space-3)');
    expect(card).toContain('--task-card-select-below: var(--ui-space-4)');
  });

  it('stretches the checkbox hit area from its outer edge to the title, whatever its border', () => {
    const target = rule(cards, '.task-card-select::after');
    expect(target).toContain('left: 50%');
    expect(target).toContain(
      'width: calc(var(--task-card-select-size) + var(--task-card-select-gap))'
    );
    expect(target).toContain('translate( calc(var(--task-card-select-size) / -2)');
    expect(target).not.toContain('inset');
  });

  it('keeps the drag placeholder muted and dashed in the dark theme and under the pointer', () => {
    const placeholder = rule(
      cards,
      ".task-card-dragging,\n.task-card-dragging:hover,\n:root[data-theme='dark'] .task-card-dragging,\n:root[data-theme='dark'] .task-card-dragging:hover"
    );
    expect(placeholder).toContain('background: var(--ui-bg-muted)');
    expect(placeholder).toContain('border-style: dashed');
    expect(placeholder).toContain('border-color: var(--ui-border-strong)');
  });

  it('gives the card title one rule, shared by the card and the copy that is dragged', () => {
    const title = rule(cards, '.task-card-title');
    expect(title).toContain('font-size: var(--ui-text-sm)');
    expect(title).toContain('line-clamp: 2');
    expect(rule(cards, '.task-card-open')).not.toContain('font-size');
    expect(cards).not.toContain('.task-card-overlay .task-card-heading');
  });

  it('keeps the card details on one line that never wraps', () => {
    const details = rule(cards, '.task-card-meta');
    expect(details).toContain('white-space: nowrap');
    expect(details).not.toContain('flex-wrap');
    const project = rule(cards, '.task-card-project');
    expect(project).toContain('min-width: 0');
    expect(project).toContain('text-overflow: ellipsis');
    expect(rule(cards, '.task-card-fact')).toContain('flex-shrink: 0');
    expect(cards).not.toContain('.task-card-agentic::before');
  });

  it('lays each link on a card out as one row behind a shared label column', () => {
    expect(rule(cards, '.task-card-links')).toContain('--task-card-link-label: 4rem');
    expect(rule(cards, '.task-card-link-label,\n.task-card .task-relation-label')).toContain(
      'flex: 0 0 var(--task-card-link-label)'
    );
    expect(rule(cards, '.task-card .task-relation')).toContain('flex-direction: row');
    expect(rule(cards, '.task-card-pull-request')).toContain('min-height: var(--ui-space-6)');
  });

  it('stacks a relation into rows only while it is expanded', () => {
    const relations = read(join(boardStyles, 'Relation.css'));
    expect(rule(cards, '.task-card .task-relation-list')).toContain('flex-direction: row');
    const expanded = rule(cards, '.task-card .task-relation[data-expanded] .task-relation-list');
    expect(expanded).toContain('flex-direction: column');
    expect(expanded).toContain('align-items: stretch');
    const toggle = rule(relations, '.task-relation-toggle');
    expect(toggle).toContain('z-index: 1');
    expect(toggle).toContain('min-width: var(--ui-space-6)');
    expect(toggle).toContain('min-height: var(--ui-space-6)');
    expect(rule(relations, '.task-relation-toggle:focus-visible')).toContain(
      'outline: 2px solid var(--ui-border-focus)'
    );
  });

  it('gives the title the full width of the card unless the menu is always shown', () => {
    expect(rule(cards, '.task-card-heading')).not.toContain('padding-right');
    expect(rule(cards, '.task-card-heading:has(.task-title-edit)')).toContain(
      'padding-right: var(--ui-control-height-sm)'
    );
    const touch = media(cards, '(any-pointer: coarse)');
    expect(rule(touch, '.task-card-heading')).toContain(
      'padding-right: var(--ui-control-height-sm)'
    );
    expect(rule(cards, '.task-card-menu')).toContain('background: var(--task-card-surface)');
    expect(rule(cards, '.task-card-menu::before')).toContain(
      'linear-gradient(to right, transparent, var(--task-card-surface))'
    );
    expect(rule(cards, ":root[data-theme='dark'] .task-card:hover")).toContain(
      '--task-card-surface: var(--ui-bg-hover)'
    );
    expect(rule(touch, '.task-card-menu::before')).toContain('content: none');
    expect(rule(cards, '.task-card:has(.task-title-edit) .task-card-menu::before')).toContain(
      'content: none'
    );
  });

  it('tints the pull request icon by status and leaves the rest of the row quiet', () => {
    expect(rule(cards, '.task-card-pull-request .pull-request-icon')).toContain(
      'color: var(--ui-text-muted)'
    );
    for (const [status, colour] of [
      ['open', '--ui-success-600'],
      ['merged', '--ui-accent'],
      ['closed', '--ui-error-600'],
    ]) {
      expect(rule(cards, `.task-card-pull-request-${status} .pull-request-icon`)).toContain(
        `color: var(${colour})`
      );
    }
    expect(cards).not.toContain('.task-card-pull-request-pending');
    expect(cards).not.toContain('.task-card-pull-request-untinted');
  });

  it('makes room above a focused card for its ring without moving the card', () => {
    expect(rule(columns, '.board-column')).toContain('--board-column-ring: 3px');
    expect(rule(columns, '.board-column-body')).toContain(
      'padding: 0 var(--ui-space-2) var(--ui-space-2)'
    );
    const focused = rule(columns, '.board-column-body:has(.task-card-open:focus-visible)');
    expect(focused).toContain('margin-top: calc(-1 * var(--board-column-ring))');
    expect(focused).toContain('padding-top: var(--board-column-ring)');
    expect(rule(cards, '.task-card-open:focus-visible::after')).toContain(
      '0 0 0 var(--board-column-ring) var(--ui-border-focus)'
    );
    expect(rule(columns, '.board-column:focus-visible')).toContain('outline-offset: -2px');
  });

  it('contains only vertical overscroll in a column, so a sideways swipe reaches the board', () => {
    const body = rule(columns, '.board-column-body');
    expect(body).toContain('overscroll-behavior-y: contain');
    expect(body).not.toContain('overscroll-behavior: contain');
  });

  it('names the board skeleton apart from every other skeleton', () => {
    expect(board).not.toMatch(/\.skeleton\b/);
    expect(board).not.toMatch(/[\s.]skeleton-(title|text|badge|card|meta|tag|count)/);
    expect(rule(board, '.board-skeleton-title')).toContain('max-width: 75%');
    expect(rule(board, '.task-card.board-skeleton-card')).toContain('pointer-events: none');
  });

  it('stops the drop highlight fading in when motion is reduced', () => {
    expect(rule(columns, '.board-column-body')).toContain('transition:');
    const reduced = media(columns, '(prefers-reduced-motion: reduce)');
    expect(rule(reduced, '.board-column-body')).toContain('transition: none');
  });

  it('snaps one column at a time on a phone', () => {
    const phone = media(board, '(max-width: 768px)');
    expect(rule(phone, '.board')).toContain('scroll-snap-type: x mandatory');
    expect(rule(phone, '.board[data-dragging]')).toContain('scroll-snap-type: none');
    const column = rule(media(columns, '(max-width: 768px)'), '.board-column');
    expect(column).toContain('scroll-snap-align: start');
    expect(column).toContain('width: min(85vw, 300px)');
  });

  it('leaves no table or table skeleton rules behind', () => {
    for (const selector of [
      '.tasks-table',
      '.task-card-agentic td',
      '.task-actions',
      '.task-delete',
      '.task-badges',
      '.task-project',
      '.task-priority',
      '.task-model',
      '.task-source',
      '.task-pr',
      '.task-branch',
      '.task-rename',
      '.task-card-title',
      '.task-card .task-description',
      '.tasks-page .skeleton',
    ]) {
      expect(css).not.toContain(selector);
    }
  });

  it('keeps the task styles on the palette', () => {
    for (const styles of [css, board, columns, cards]) {
      expect(styles).not.toMatch(legacyScales);
    }
  });

  it('wraps the task filters instead of shrinking them off the page bar', () => {
    const phone = media(css, '(max-width: 768px)');
    expect(rule(phone, '.tasks-filters')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.tasks-filters .ui-select-trigger')).toContain('width: auto');
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

  it('sizes the editor chrome to the header', () => {
    expect(rule(css, '.task-details-header')).toContain('height: var(--ui-header-height)');
    expect(rule(css, '.task-details-actions')).toContain('height: var(--ui-header-height)');
  });
});

describe('bulk edit dialog', () => {
  const kit = join(import.meta.dir, '..', '..', '..', '..', 'packages', 'ui', 'src', 'styles');
  const globals = read(join(kit, 'globals.css'));
  const variables = read(join(kit, 'variables.css'));
  const dialog = read(join(features, 'tasks', 'components', 'EditTasksDialog.css'));
  const ROOT_FONT_SIZE = 16;

  function pixels(length: string): number {
    const value = Number.parseFloat(length);
    return length.endsWith('rem') ? value * ROOT_FONT_SIZE : value;
  }

  function declaration(block: string, property: string): string {
    const match = block.match(new RegExp(`(?:^|; )${property}: ([^;]+)`));
    if (!match) throw new Error(`${property} is not declared`);
    return match[1].trim();
  }

  const focus = rule(globals, '.ui-checkbox:focus-visible');
  const ring =
    pixels(declaration(focus, 'outline').split(' ')[0]) +
    pixels(declaration(focus, 'outline-offset'));

  it('pads the scrolling project list sideways so a focused checkbox shows its whole ring', () => {
    const list = rule(dialog, '.edit-tasks-project-list');
    expect(list).toContain('overflow-y: auto');
    expect(declaration(list, 'padding-inline')).toBe('var(--ui-space-1)');
    expect(declaration(list, 'margin-inline')).toBe('calc(var(--ui-space-1) * -1)');
    expect(pixels(token(variables, 'ui-space-1'))).toBeGreaterThanOrEqual(ring);
  });

  it('leaves room above and below a centred checkbox for its ring', () => {
    expect(rule(dialog, '.edit-tasks-project-list .ui-checkbox-wrapper')).toContain(
      'align-items: center'
    );
    const row = pixels(token(variables, 'ui-control-height-sm'));
    const box = pixels(declaration(rule(globals, '.ui-checkbox'), 'height'));
    expect((row - box) / 2).toBeGreaterThanOrEqual(ring);
  });
});

describe('projects page layout', () => {
  const css = read(join(features, 'projects', 'pages', 'ProjectsPage.css'));

  it('draws no gradient behind the page and leaves pane width to SubNav', () => {
    expect(css).not.toContain('radial-gradient');
    expect(rule(css, '.projects-list-pane')).not.toContain('width:');
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

describe('pull requests page layout', () => {
  const pages = join(features, 'pull-requests', 'pages');
  const css = read(join(pages, 'PullRequestsPage.css'));
  const page = read(join(pages, 'PullRequestsPage.tsx'));

  it('lists pull requests in the resizable SubNav pane and leaves its width to SubNav', () => {
    expect(page).toContain('<SubNav');
    expect(page).toContain("PANE_STORAGE_KEY = 'pull-requests-pane'");
    expect(rule(css, '.pull-requests-list-pane')).not.toContain('width:');
    expect(rule(css, '.pull-requests-list')).toContain('overflow-y: auto');
  });

  it('reuses the projects detail chrome instead of redefining it', () => {
    for (const selector of [
      '.details-header',
      '.details-content',
      '.detail-facts',
      '.details-actions',
    ]) {
      expect(css).not.toContain(`${selector} {`);
    }
  });

  it('makes every list card exactly 72px on 8/12 padding with a 20 + 18 + 16 stack', () => {
    const card = rule(css, '.pull-request-card.card--list');
    expect(card).toContain('height: 4.5rem');
    expect(card).toContain('box-sizing: border-box');
    expect(card).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
    expect(card).not.toContain('gap:');
    expect(rule(css, '.pull-request-card-header')).toContain('height: var(--ui-badge-height)');
    expect(rule(css, '.pull-request-card-meta')).toContain('line-height: 1.125rem');
    expect(rule(css, '.pull-request-card-footer')).toContain('height: var(--ui-space-4)');
  });

  it('sticks the readiness group headers inside the list scroller at 28px', () => {
    const header = rule(css, '.pull-request-group-header');
    expect(header).toContain('position: sticky');
    expect(header).toContain('top: 0');
    expect(header).toContain('height: var(--ui-control-height-sm)');
    expect(header).toContain('background: var(--ui-bg-base)');
    expect(rule(css, '.pull-request-group')).not.toContain('overflow');
  });

  it('sets blockers on a 20px chip row and checks, commits and diff files on 40px rows', () => {
    expect(rule(css, '.pull-request-blocker')).toContain('min-height: var(--ui-badge-height)');
    expect(rule(css, '.pull-request-row')).toContain('min-height: var(--ui-list-row)');
    expect(rule(css, '.pull-request-commit')).toContain('height: var(--ui-list-row)');
    expect(rule(css, '.pull-request-diff-file-header')).toContain('height: var(--ui-list-row)');
  });

  it('lays diff lines on a mono 20px grid that wraps instead of scrolling the page sideways', () => {
    const line = rule(css, '.pull-request-diff-line');
    expect(line).toContain('display: grid');
    expect(line).toContain('grid-template-columns: 2.5rem 2.5rem 1rem minmax(0, 1fr)');
    expect(line).toContain('font-family: var(--ui-font-mono)');
    expect(line).toContain('font-size: var(--ui-text-xs)');
    expect(line).toContain('line-height: var(--ui-space-5)');
    expect(rule(css, '.pull-request-diff-content')).toContain('white-space: pre-wrap');
    expect(rule(css, '.pull-request-diff-content')).toContain('overflow-wrap: anywhere');
  });

  it('tints diff lines and notices from the palette', () => {
    expect(rule(css, ".pull-request-diff-line[data-kind='addition']")).toContain(
      'color-mix(in srgb, var(--ui-success) 12%, transparent)'
    );
    expect(rule(css, ".pull-request-diff-line[data-kind='deletion']")).toContain(
      'color-mix(in srgb, var(--ui-error) 12%, transparent)'
    );
    expect(rule(css, ":root[data-theme='dark'] .pull-request-diff-additions")).toContain(
      'var(--ui-success-400)'
    );
  });

  it('keeps every colour on the palette', () => {
    expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(css).not.toContain('rgba(');
    expect(css).not.toMatch(legacyScales);
    expect(css).not.toContain('translateY');
    expect(css).not.toContain('gradient(');
    for (const name of css.match(/var\(--[a-z0-9-]+/g) ?? []) {
      expect(name).toStartWith('var(--ui-');
    }
  });

  it('leaves the skeleton width to SubNav', () => {
    expect(rule(css, '.pull-requests-skeleton')).not.toMatch(/(^|[^-])width:/m);
    expect(page).toContain('listPane(<PullRequestsSkeleton />)');
  });

  it('lets the filter selects give way before the subtitle is cut short', () => {
    const control = rule(css, '.pull-requests-filters .ui-select-control');
    expect(control).toContain('flex: 0 1 11rem');
    expect(control).toContain('min-width: 7rem');
    expect(rule(css, '.pull-requests-filters .ui-select-trigger')).toContain('width: 100%');
  });

  it('hides the list pane on a phone while a pull request is open', () => {
    const phone = media(css, '(max-width: 768px)');
    expect(
      rule(
        phone,
        '.pull-requests-workspace:has(.pull-request-details) .pull-requests-list-pane,\n  .pull-requests-detail-placeholder'
      )
    ).toContain('display: none');
    expect(rule(phone, '.pull-requests-filters')).toContain('flex-wrap: wrap');
  });
});

describe('sources section layout', () => {
  const css = read(join(features, 'sources', 'components', 'SourcesSection.css'));

  it('lays sources out as a 56px-row table with a 32px header', () => {
    expect(rule(css, '.sources-table-wrapper')).toContain('border: 1px solid var(--ui-border)');
    expect(rule(css, '.sources-table th')).toContain('height: var(--ui-control-height)');
    expect(rule(css, '.sources-table td')).toContain('height: var(--ui-list-row-2)');
    expect(rule(css, '.sources-table td')).toContain('box-shadow: inset 0 -1px var(--ui-border)');
    expect(rule(css, '.source-name')).toContain('line-height: var(--ui-space-5)');
    expect(rule(css, '.source-description')).toContain('height: var(--ui-space-4)');
    expect(rule(css, '.source-url')).toContain('line-height: var(--ui-space-4)');
  });

  it('makes source rows clickable and lays the editor out as a 48px header and footer', () => {
    expect(rule(css, '.sources-table tbody .source-card')).toContain('cursor: pointer');
    expect(rule(css, '.source-details-header')).toContain('height: var(--ui-header-height)');
    expect(rule(css, '.source-details-header')).toContain('padding: 0 var(--ui-gutter)');
    expect(rule(css, '.source-details-body')).toContain(
      'padding: var(--ui-panel-padding) var(--ui-gutter)'
    );
    expect(rule(css, '.source-details-actions')).toContain('height: var(--ui-header-height)');
    expect(rule(css, '.source-details-header h3')).toContain('font-size: var(--ui-heading-size)');
  });

  it('sticks the editor footer to the settings scroller instead of scrolling a body of its own', () => {
    const footer = rule(css, '.source-details-actions');
    expect(footer).toContain('position: sticky');
    expect(footer).toContain('bottom: 0;');
    expect(footer).toContain('background: var(--ui-bg-elevated)');
    expect(footer).toContain('border-radius: 0 0 var(--ui-radius-card) var(--ui-radius-card)');
    expect(css).not.toContain('.page-body');
    expect(rule(css, '.source-details')).not.toContain('flex: 1');
    expect(rule(css, '.source-details')).not.toContain('min-height');
    expect(rule(css, '.source-details-body')).not.toContain('overflow');
    expect(rule(css, '.source-details-body')).not.toContain('flex: 1');
  });

  it('shows source form hints in full over the two-line settings clamp in any import order', () => {
    const settings = read(join(features, 'settings', 'pages', 'SettingsPage.css'));
    expect(rule(settings, '.settings-page .form-hint')).toContain('display: -webkit-box');
    const hint = rule(css, '.settings-page .source-details .form-hint');
    expect(hint).toContain('display: block');
    expect(hint).toContain('overflow: visible');
    expect(hint).toContain('-webkit-line-clamp: unset');
  });

  it('leaves the empty state to sit in the section flow like its sibling sections', () => {
    expect(css).not.toContain('.sources-empty');
  });

  it('aligns the access token disclosure with the field labels', () => {
    expect(rule(css, '.github-configuration > .ui-btn-link')).toContain('padding-inline: 0');
  });

  it('drops the URL and verified columns when the settings column is too narrow for the actions', () => {
    expect(rule(css, '.sources-table-wrapper')).toContain('container-type: inline-size');
    const desktop = media(css, '(min-width: 769px)');
    expect(desktop).toContain('@container (max-width: 48rem)');
    expect(
      rule(
        desktop,
        '.sources-table th:nth-child(4),\n.sources-table td:nth-child(4),\n.sources-table th:nth-child(5),\n.sources-table td:nth-child(5)'
      )
    ).toContain('display: none');
    const name = rule(desktop, '.sources-table td:first-child');
    expect(name).toContain('width: 100%');
    expect(name).toContain('max-width: 0');
  });

  it('lets the URL column absorb the spare width so the actions fit the settings column', () => {
    const cell = rule(css, '.sources-table .source-url-cell');
    expect(cell).toContain('width: 100%');
    expect(cell).toContain('max-width: 0');
  });
});

describe('settings page layout', () => {
  const css = read(join(features, 'settings', 'pages', 'SettingsPage.css'));
  const section = read(join(features, 'settings', 'server', 'components', 'FeaturesSection.tsx'));

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

  it('pads the scroller under the source editor footer so a focused field scrolls clear of it', () => {
    const body = rule(css, '.settings-page .page-body:has(.source-details-actions)');
    expect(body).toContain('padding-bottom: 0');
    expect(body).toContain(
      'scroll-padding-bottom: calc(var(--ui-header-height) + var(--ui-space-2))'
    );
  });

  it('keeps feature setup in a settings section without a content card', () => {
    const panel = read(join(features, 'models', 'components', 'FeaturesPanel.css'));
    expect(panel).not.toContain('.card');
    expect(rule(panel, '.models-feature')).toContain('border-bottom: 1px solid var(--ui-border)');
    expect(section).toContain('<FeaturesPanel');
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
    expect(page).not.toContain('value="features"');
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

  it('sends the empty state to the server features section', () => {
    expect(page).not.toContain('<FeaturesPanel');
    expect(page).toContain('SettingsSection.Features');
    expect(page).not.toContain("'/settings?tab=features'");
  });
});

describe('first-run feature setup', () => {
  const app = read(join(import.meta.dir, '..', 'App.tsx'));
  const setup = read(join(features, 'models', 'pages', 'SetupPage.tsx'));
  const css = read(join(features, 'models', 'pages', 'SetupPage.css'));

  it('is a setup step after login that reuses the features picker', () => {
    expect(app).toContain('path="/setup"');
    expect(app).toContain('<SetupGate>');
    expect(app).toContain('<SetupPage');
    expect(setup).toContain('<FeaturesPanel');
    expect(setup).toContain('HostFoldersPanel');
    expect(
      rule(
        read(join(features, 'settings', 'workspace', 'components', 'HostFoldersPanel.css')),
        '.host-folders--setup .section-row, .host-folders--setup .section-row-copy'
      )
    ).toContain('flex-direction: column');
    expect(setup).toContain('Skip for now');
    expect(setup).toContain('Continue');
    expect(css).not.toContain('.card');
    expect(rule(css, '.setup-page')).toContain('min-height: 100dvh');
    expect(rule(css, '.setup-footer')).toContain('border-top: 1px solid var(--ui-border)');
    expect(rule(css, '.setup-page .models-feature-size--ready')).toContain(
      'color: var(--ui-success-600)'
    );
  });
});

describe('setup license list', () => {
  it('stacks each license on its own line', () => {
    const panel = read(join(features, 'models', 'components', 'FeaturesPanel.css'));
    expect(rule(panel, '.models-setup-licenses ul')).toContain('list-style: none');
    expect(rule(panel, '.models-setup-licenses li')).toContain('overflow-wrap: anywhere');
  });
});

describe('github repository picker', () => {
  const css = read(join(features, 'github', 'components', 'RepositoryPicker.css'));

  it('lays the popup out in flow so a scrolling wizard or modal body grows instead of clipping it', () => {
    const popup = rule(css, '.repository-picker-popup');
    expect(popup).not.toContain('position');
    expect(popup).toContain('max-height: min(16rem, 40dvh)');
    expect(popup).toContain('overflow-y: auto');
    expect(css).not.toContain('ui-fade-down');
  });
});

describe('releases page layout', () => {
  const css = read(join(features, 'releases', 'pages', 'ReleasesPage.css'));

  it('scrolls one page body under 28px day headers that stick inside it', () => {
    const body = rule(css, '.releases-body');
    expect(body).toContain('min-height: 0');
    expect(body).not.toContain('overflow');
    const header = rule(css, '.release-day-header');
    expect(header).toContain('position: sticky');
    expect(header).toContain('top: 0');
    expect(header).toContain('height: var(--ui-control-height-sm)');
    expect(header).toContain('background: var(--ui-bg-base)');
  });

  it('makes every release card header exactly 72px on 8/12 padding with a 20 + 18 + 16 stack', () => {
    const header = rule(css, '.release-card-header');
    expect(header).toContain('height: 4.5rem');
    expect(header).toContain('box-sizing: border-box');
    expect(header).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
    expect(header).not.toContain('gap:');
    expect(rule(css, '.release-card-title')).toContain('height: var(--ui-space-5)');
    expect(rule(css, '.release-card-meta')).toContain('height: 1.125rem');
    expect(rule(css, '.release-card-footer')).toContain('height: var(--ui-space-4)');
  });

  it('seats whole pipeline chips in a 20px badge row and lets the name give way instead', () => {
    const chips = rule(css, '.release-pipeline-chips');
    expect(chips).toContain('height: var(--ui-badge-height)');
    expect(chips).toContain('flex-shrink: 0');
    expect(chips).not.toContain('overflow');
    expect(chips).not.toContain('flex-wrap: wrap');
    expect(rule(css, '.release-card-name')).toContain('min-width: 3rem');
    const text = rule(css, '.release-card-tag-text,\n.release-pipeline-chip-text');
    expect(text).toContain('text-overflow: ellipsis');
    expect(text).toContain('white-space: nowrap');
  });

  it('keeps the expand toggle within the 16px footer line, widening its hit area only sideways', () => {
    const toggle = rule(css, '.release-card-toggle');
    expect(toggle).toContain('height: var(--ui-space-4)');
    expect(toggle).toContain('margin: 0 calc(var(--ui-space-1) * -1)');
    expect(toggle).toContain('padding: 0 var(--ui-space-1)');
    expect(toggle).toContain('line-height: var(--ui-space-4)');
  });

  it('lists included pull requests as 40px rows with a mono number', () => {
    expect(rule(css, '.release-card-pull')).toContain('height: var(--ui-list-row)');
    const number = rule(css, '.release-card-pull-number');
    expect(number).toContain('font-family: var(--ui-font-mono)');
    expect(number).toContain('font-size: var(--ui-text-xs)');
    expect(number).toContain('line-height: var(--ui-space-5)');
    expect(rule(css, '.release-card-pulls[hidden]')).toContain('display: none');
  });

  it('caps the release column at the content width', () => {
    expect(rule(css, '.release-day')).toContain('max-width: var(--ui-content-width)');
  });

  it('wraps long links in the release notes instead of scrolling them sideways', () => {
    const notes = rule(css, '.release-wizard-notes');
    expect(notes).toContain('overflow-wrap: anywhere');
    expect(notes).toContain('overflow-y: auto');
  });

  it('paints only from the palette, with no lifts or decoration', () => {
    expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(css).not.toContain('rgba(');
    expect(css).not.toContain('rgb(');
    expect(css).not.toContain('--gray-');
    expect(css).not.toContain('translateY');
    expect(css).not.toContain('::before');
    expect(css).not.toContain('gradient(');
    for (const name of css.match(/var\(--[a-z0-9-]+/g) ?? []) {
      expect(name).toStartWith('var(--ui-');
    }
    expect(css).toContain(":root[data-theme='dark'] .release-day-count.ui-badge-neutral");
  });

  it('never restyles the page bar', () => {
    expect(css).not.toContain('.page-bar');
  });
});

describe('github source chrome', () => {
  const components = join(features, 'github', 'components');
  const shimmer = read(join(components, 'Shimmer.css'));
  const notices = read(join(components, 'SourceNotices.css'));

  it('draws the one skeleton shimmer both pages share, and stills it for reduced motion', () => {
    expect(rule(shimmer, '.source-shimmer')).toContain('linear-gradient');
    expect(rule(shimmer, '.source-shimmer')).toContain('animation: ui-shimmer');
    expect(media(shimmer, '(prefers-reduced-motion: reduce)')).toContain('animation: none');
  });

  it('tints notices from the palette', () => {
    expect(rule(notices, '.source-notice')).toContain(
      'color-mix(in srgb, var(--ui-warning) 12%, transparent)'
    );
    for (const css of [shimmer, notices]) {
      expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
      for (const name of css.match(/var\(--[a-z0-9-]+/g) ?? []) {
        expect(name).toStartWith('var(--ui-');
      }
    }
  });
});
