import { describe, expect, it } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { media, read, rule } from '../../../test/css';
import { findSection, SettingsGroup, SettingsSection } from '../../settings/navigation/sections';
import * as labels from './labels';
import {
  actionLabel,
  budgetsDescription,
  deleteMessage,
  refusalMessage,
  scopeLabel,
  stateLabel,
  uncountedLine,
} from './messages';
import { BUDGET_SCOPES, BUDGET_STATES, SPEND_REFUSAL_CODES, UNCOUNTED_REASONS } from './schemas';

const css = read(join(import.meta.dir, 'pages', 'SpendPage.css'));

const PHONE = '(max-width: 768px)';

const SOURCE = /\.tsx?$/;

const TEST = /\.test\.tsx?$/;

const PLAN_WORD = /Team/;

const FORBIDDEN_PLAN = /enterprise/i;

const EDITION_READ = /\.edition\b|\bedition\s*[=!]==|Edition\.\w+/;

function sources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sources(path);
    return SOURCE.test(entry.name) && !TEST.test(entry.name) ? [path] : [];
  });
}

const STRING_LITERAL = /'(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*"|`(?:[^`\\]|\\.)*`/g;

const IMPORT_LINE = /^\s*(?:import|export)\s.*from\s+'[^']+';?$/gm;

function copyIn(path: string): string[] {
  const code = readFileSync(path, 'utf8').replace(IMPORT_LINE, '');
  return [...code.matchAll(STRING_LITERAL)].map((match) => match[0]);
}

const COPY: string[] = [
  ...Object.values(labels),
  ...SPEND_REFUSAL_CODES.map(refusalMessage),
  ...UNCOUNTED_REASONS.map(uncountedLine),
  ...BUDGET_SCOPES.map(scopeLabel),
  ...BUDGET_STATES.map(stateLabel),
  budgetsDescription('2026-11'),
  deleteMessage('Platform'),
  actionLabel(labels.EDIT_LABEL, 'Platform'),
];

describe('spend copy', () => {
  it('never names the Enterprise plan', () => {
    for (const text of COPY) {
      expect(text).not.toMatch(FORBIDDEN_PLAN);
    }
  });

  it('never names the Enterprise plan in any string a spend module renders', () => {
    for (const path of sources(import.meta.dir)) {
      if (path.endsWith('fixtures.ts')) continue;
      for (const literal of copyIn(path)) {
        expect(literal, relative(import.meta.dir, path)).not.toMatch(FORBIDDEN_PLAN);
      }
    }
  });

  it('names the Team plan wherever it names a plan', () => {
    for (const text of [
      labels.UPGRADE_NOTICE,
      labels.LAPSED_NOTICE,
      refusalMessage('license_required'),
    ]) {
      expect(text).toMatch(PLAN_WORD);
    }
  });

  it('says plainly what the report never counts', () => {
    expect(labels.UNCOUNTED_FOOTNOTE).toStartWith('Not counted:');
    for (const piece of [
      'coding-agent subscriptions',
      'direct provider keys',
      'other endpoints',
      "workspace keys saved for this server's LiteLLM",
      'chats that pick their own provider',
      'requests outside a workspace',
      'embeddings',
      '$0 unless LiteLLM prices them',
    ]) {
      expect(labels.UNCOUNTED_FOOTNOTE).toContain(piece);
    }
  });

  it('names the AI settings link the way the Settings navigation does', () => {
    const section = findSection(SettingsGroup.Organization, SettingsSection.Ai);
    expect(section).toBeDefined();
    expect(labels.AI_SETTINGS_LINK).toEndWith(`Settings › ${section?.label}`);
  });

  it('tells admins that hard limits never stop coding agents or direct providers', () => {
    expect(labels.HARD_LIMIT_NOTE).toBe(
      "Hard limits stop requests through Zone's LiteLLM only. Coding agents and direct providers keep running."
    );
  });
});

describe('spend license handling', () => {
  it('never branches on the edition, so a new plan needs no console change', () => {
    for (const path of sources(import.meta.dir)) {
      if (path.endsWith('fixtures.ts')) continue;
      expect(readFileSync(path, 'utf8'), relative(import.meta.dir, path)).not.toMatch(EDITION_READ);
    }
  });
});

describe('spend page surfaces', () => {
  it('draws only from the design tokens', () => {
    const variables = [...css.matchAll(/var\((--[\w-]+)/g)].map((match) => match[1]);
    expect(variables.length).toBeGreaterThan(0);
    for (const variable of variables) {
      expect(variable).toStartWith('--ui-');
    }
  });

  it('has no color literals', () => {
    expect(css).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(css).not.toMatch(/\b(?:rgb|rgba|hsl|hsla|oklch|color-mix)\(/i);
    expect(css).not.toMatch(/:\s*(?:white|black)\b/i);
  });

  it('lets every column shrink below its content', () => {
    for (const selector of [
      '.spend-page',
      '.spend-notice',
      '.spend-range',
      '.spend-range-fields',
      '.spend-overview',
      '.spend-tiles',
      '.spend-grid',
      '.spend-breakdown',
      '.spend-uncounted',
      '.spend-budget',
      '.spend-budget-header',
      '.spend-budget-name',
      '.spend-budget-footer',
      '.spend-editor',
      '.spend-editor-fields',
    ]) {
      expect(rule(css, selector), selector).toContain('min-width: 0');
    }
  });

  it('wraps long names instead of scrolling sideways', () => {
    expect(rule(css, '.spend-budget-name')).toContain('overflow-wrap: anywhere');
    expect(rule(css, '.spend-budget-header')).toContain('flex-wrap: wrap');
    expect(rule(css, '.spend-range-fields')).toContain(
      'grid-template-columns: repeat(auto-fit, minmax(min(100%, 10rem), 1fr))'
    );
  });

  it('stacks the breakdowns once two would not fit side by side', () => {
    expect(rule(css, '.spend-grid')).toContain(
      'grid-template-columns: repeat(auto-fit, minmax(min(100%, 28rem), 1fr))'
    );
  });

  it('marks each uncounted reason as a list item', () => {
    expect(rule(css, '.spend-uncounted-list')).toContain('list-style: disc outside');
  });

  it('colors each budget meter by its state', () => {
    expect(rule(css, '.spend-meter-fill')).toContain('background: var(--ui-success)');
    expect(rule(css, ".spend-budget[data-state='warning'] .spend-meter-fill")).toContain(
      'background: var(--ui-warning)'
    );
    expect(rule(css, ".spend-budget[data-state='exhausted'] .spend-meter-fill")).toContain(
      'background: var(--ui-error)'
    );
  });

  it('fills the daily bars from the accent', () => {
    expect(rule(css, '.spend-chart-fill, .spend-chart-swatch')).toContain('fill: var(--ui-accent)');
  });

  it('stacks the breakdowns, drops the token column and stacks actions on a phone', () => {
    const phone = media(css, PHONE);
    expect(rule(phone, '.spend-grid')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.spend-tokens')).toContain('display: none');
    expect(rule(phone, '.spend-actions')).toContain('flex-direction: column-reverse');
  });
});
