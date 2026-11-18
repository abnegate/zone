import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { ReviewThreads } from './ReviewThreads';

describe('ReviewThreads', () => {
  it('renders the comment count beside the open and resolved review thread counts', () => {
    render(<ReviewThreads comments={2} threads={{ open: 0, resolved: 0 }} />);

    const counts = screen.getByRole('img', {
      name: '2 comments, 0 open review threads, 0 resolved',
    });
    expect(counts.querySelector('.chat-change-threads-comments')).toHaveTextContent('2');
    expect(counts.querySelector('.chat-change-threads-resolved')).toHaveTextContent('0');
  });

  it('renders a review thread count of zero rather than hiding it', () => {
    render(<ReviewThreads comments={null} threads={{ open: 0, resolved: 0 }} />);

    expect(
      screen.getByRole('img', { name: '0 open review threads, 0 resolved' })
    ).toBeInTheDocument();
  });

  it('renders the comment count alone when no review threads were read', () => {
    render(<ReviewThreads comments={1} threads={null} />);

    const counts = screen.getByRole('img', { name: '1 comment' });
    expect(counts.querySelector('.chat-change-threads-comments')).toHaveTextContent('1');
    expect(counts.querySelector('.chat-change-threads-open')).toBeNull();
    expect(counts.querySelector('.chat-change-threads-resolved')).toBeNull();
  });

  it('renders nothing when neither count was ever read', () => {
    render(<ReviewThreads comments={null} threads={null} />);

    expect(screen.queryByRole('img')).toBeNull();
  });

  it('marks an open review thread with the pending colour class', () => {
    render(<ReviewThreads comments={null} threads={{ open: 2, resolved: 1 }} />);

    const counts = screen.getByRole('img', { name: '2 open review threads, 1 resolved' });
    expect(counts.querySelector('.chat-change-threads-open')).toHaveTextContent('2');
  });
});
