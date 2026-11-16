import { describe, expect, it } from 'bun:test';
import { render } from '@testing-library/react';
import { MeterRow } from './MeterRow';

function meter(value: number, maximum: number) {
  const { container } = render(
    <ul>
      <MeterRow
        label="Build"
        value={value}
        maximum={maximum}
        display={String(value)}
        tone="destructive"
        detail="2 tasks"
      />
    </ul>
  );
  return container.querySelector('.analytics-meter') as HTMLElement;
}

describe('MeterRow', () => {
  it('fills the track in proportion to the largest value', () => {
    const row = meter(3, 12);

    expect(row.getAttribute('data-tone')).toBe('destructive');
    expect((row.querySelector('.analytics-meter-fill') as HTMLElement).style.inlineSize).toBe(
      '25%'
    );
    expect(row.querySelector('.analytics-meter-value')?.textContent).toBe('3');
    expect(row.querySelector('.analytics-meter-detail')?.textContent).toBe('2 tasks');
  });

  it('draws an empty track when nothing was counted', () => {
    const row = meter(0, 0);

    expect((row.querySelector('.analytics-meter-fill') as HTMLElement).style.inlineSize).toBe('0%');
  });

  it('stops the fill at the end of the track', () => {
    const row = meter(15, 10);

    expect((row.querySelector('.analytics-meter-fill') as HTMLElement).style.inlineSize).toBe(
      '100%'
    );
  });

  it('keeps the track out of the accessible text', () => {
    const row = meter(3, 12);

    expect(row.querySelector('.analytics-meter-track')?.getAttribute('aria-hidden')).toBe('true');
  });
});
