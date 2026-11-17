import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { AnalyticsSkeleton, LOADING_LABEL } from './AnalyticsSkeleton';

describe('AnalyticsSkeleton', () => {
  it('announces that analytics are loading', () => {
    render(<AnalyticsSkeleton />);

    expect(screen.getByRole('status', { name: LOADING_LABEL })).toBeDefined();
  });
});
