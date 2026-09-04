import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { BrowseModelSchema } from '../schemas';
import Capabilities from './Capabilities';

describe('Capabilities', () => {
  it('labels unavailable metadata explicitly', () => {
    render(<Capabilities />);
    expect(screen.getByText('Capabilities unknown')).toBeInTheDocument();
  });

  it('preserves exact typed capabilities in browse responses', () => {
    const model = BrowseModelSchema.parse({
      name: 'model',
      capabilities: ['text', 'image_input', 'tools'],
    });
    expect(model.capabilities).toEqual(['text', 'image_input', 'tools']);
    expect(BrowseModelSchema.safeParse({ name: 'model', capabilities: ['guessed'] }).success).toBe(
      false
    );
    expect(BrowseModelSchema.parse({ name: 'model' }).capabilities).toBeUndefined();
  });

  it('deduplicates declared labels', () => {
    render(<Capabilities capabilities={['tools', 'tools']} />);
    expect(screen.getAllByText('Tools')).toHaveLength(1);
  });
});
