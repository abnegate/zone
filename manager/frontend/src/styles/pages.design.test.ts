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
