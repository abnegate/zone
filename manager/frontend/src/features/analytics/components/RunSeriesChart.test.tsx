import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import { chartCaption, RunSeriesChart } from './RunSeriesChart';

const { series } = overview().runs;

function chart() {
  return render(<RunSeriesChart series={series} period="week" />);
}

describe('RunSeriesChart', () => {
  it('titles the chart for assistive technology', () => {
    chart();

    const image = screen.getByRole('img', { name: chartCaption('week') });
    expect(image.tagName.toLowerCase()).toBe('svg');
    expect(image.querySelector('title')?.textContent).toBe(chartCaption('week'));
  });

  it('stacks only the outcomes a bucket saw', () => {
    const { container } = chart();

    const bars = container.querySelectorAll('.analytics-chart-bar');
    expect(bars).toHaveLength(series.length);
    expect(bars[0].querySelectorAll('.analytics-series')).toHaveLength(2);
    expect(bars[2].querySelectorAll('.analytics-series')).toHaveLength(0);
    expect(bars[3].querySelectorAll('.analytics-series')).toHaveLength(3);
  });

  it('scales the tallest bar against a rounded axis', () => {
    const { container } = chart();

    const tallest = container.querySelectorAll('.analytics-chart-bar')[5];
    const heights = [...tallest.querySelectorAll('.analytics-series')].map((segment) =>
      Number(segment.getAttribute('height'))
    );
    expect(heights.reduce((sum, height) => sum + height, 0)).toBeCloseTo(90);
    expect(container.querySelector('.analytics-chart-axis')?.textContent).toBe('1050');
  });

  it('paints each outcome from the theme', () => {
    const { container } = chart();

    const classes = [...container.querySelectorAll('.analytics-chart-bar')[3].children]
      .filter((element) => element.classList.contains('analytics-series'))
      .map((element) => element.getAttribute('class'));
    expect(classes).toEqual([
      'analytics-series analytics-series--succeeded',
      'analytics-series analytics-series--failed',
      'analytics-series analytics-series--cancelled',
    ]);
  });

  it('describes each bucket in a hover title', () => {
    const { container } = chart();

    const titles = [...container.querySelectorAll('.analytics-chart-bar > title')].map(
      (title) => title.textContent
    );
    expect(titles[0]).toMatch(/4 runs, 3 succeeded, 1 failed, 0 cancelled$/);
    expect(titles[2]).toMatch(/no runs$/);
  });

  it('carries every bucket in a visually hidden table', () => {
    const { container } = chart();

    const table = container.querySelector('.sr-only table') as HTMLTableElement;
    expect(within(table).getByText(chartCaption('week'))).toBeDefined();
    const rows = table.querySelectorAll('tbody tr');
    expect(rows).toHaveLength(series.length);
    expect([...rows[3].querySelectorAll('td')].map((cell) => cell.textContent)).toEqual([
      '8',
      '6',
      '1',
      '1',
    ]);
  });

  it('totals each outcome in the legend', () => {
    const { container } = chart();

    const legend = container.querySelector('.analytics-legend')?.textContent;
    expect(legend).toContain('Succeeded30');
    expect(legend).toContain('Failed8');
    expect(legend).toContain('Cancelled2');
  });
});
