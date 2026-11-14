import { describe, expect, it } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Checkbox, type CheckedState } from '.';

function render(checked: CheckedState): string {
  return renderToStaticMarkup(
    <Checkbox aria-label="Project" checked={checked} onCheckedChange={() => {}} />
  );
}

describe('Checkbox', () => {
  it('announces an indeterminate box as mixed and draws a dash', () => {
    const markup = render('indeterminate');

    expect(markup).toContain('role="checkbox"');
    expect(markup).toContain('aria-checked="mixed"');
    expect(markup).toContain('data-state="indeterminate"');
    expect(markup).toContain('<line');
    expect(markup).not.toContain('<polyline');
  });

  it('announces a checked box as checked and draws a tick', () => {
    const markup = render(true);

    expect(markup).toContain('aria-checked="true"');
    expect(markup).toContain('data-state="checked"');
    expect(markup).toContain('<polyline');
    expect(markup).not.toContain('<line');
  });

  it('draws no mark on an unchecked box', () => {
    const markup = render(false);

    expect(markup).toContain('aria-checked="false"');
    expect(markup).toContain('data-state="unchecked"');
    expect(markup).not.toContain('<svg');
  });

  it('keeps the square control class so the design-system styles apply', () => {
    expect(render(false)).toContain('class="ui-checkbox"');
  });
});
