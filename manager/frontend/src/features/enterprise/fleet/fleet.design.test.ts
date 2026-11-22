import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule } from '../../../test/css';

const runners = read(join(import.meta.dir, 'components', 'RunnersSection.css'));

const placement = read(join(import.meta.dir, 'components', 'RunnerPlacementCard.css'));

const PHONE = '(max-width: 768px)';

describe.each([
  ['runners', runners],
  ['runner placement', placement],
])('%s surfaces', (_, css) => {
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
});

describe('runners section', () => {
  it('lets every column shrink below its content', () => {
    for (const selector of [
      '.runners-section',
      '.runners-minimum, .runners-table-card',
      '.runners-minimum-form',
      '.runner-stack',
      '.runner-line',
      '.runner-labels',
    ]) {
      expect(rule(runners, selector), selector).toContain('min-width: 0');
    }
  });

  it('keeps versions and labels whole in the table and breaks them only on a phone', () => {
    expect(rule(runners, '.runner-version, .runner-label')).toContain(
      'font-family: var(--ui-font-mono)'
    );
    expect(rule(runners, '.runner-version, .runner-label')).toContain('white-space: nowrap');
    expect(rule(runners, '.runner-labels')).toContain('flex-wrap: wrap');
    const phone = rule(media(runners, PHONE), '.runner-name, .runner-label');
    expect(phone).toContain('white-space: normal');
    expect(phone).toContain('overflow-wrap: anywhere');
  });

  it('turns each runner into a labelled stack on a phone', () => {
    const phone = media(runners, PHONE);
    expect(rule(phone, '.runners-table thead')).toContain('display: none');
    expect(rule(phone, '.runners-table, .runners-table tbody, .runners-table tr')).toContain(
      'display: block'
    );
    expect(rule(phone, '.runners-table td')).toContain(
      'grid-template-columns: 5rem minmax(0, 1fr)'
    );
    expect(rule(phone, '.runners-table td::before')).toContain('content: attr(data-label)');
  });

  it('colours a serving runner from the success scale in both themes', () => {
    expect(rule(runners, '.runner-work--serving, .runner-seen--connected')).toContain(
      'color: var(--ui-success-600)'
    );
    expect(
      rule(
        runners,
        ":root[data-theme='dark'] .runner-work--serving, :root[data-theme='dark'] .runner-seen--connected"
      )
    ).toContain('color: var(--ui-success-400)');
  });
});

describe('runner placement card', () => {
  it('sets offered labels in the mono face and wraps them', () => {
    expect(rule(placement, '.runner-placement-option-label')).toContain(
      'font-family: var(--ui-font-mono)'
    );
    expect(rule(placement, '.runner-placement-options')).toContain('flex-wrap: wrap');
    expect(rule(placement, '.runner-placement-option')).toContain('max-width: 100%');
  });

  it('sets inherited labels apart from the ones a workspace can remove', () => {
    expect(rule(placement, '.runner-placement-chip--locked')).toContain('border-style: dashed');
  });
});
