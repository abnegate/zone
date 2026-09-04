import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import type { ActionReceipt } from '../types';
import { ActionReceipts } from './ActionReceipts';

const receipt = (overrides: Partial<ActionReceipt> = {}): ActionReceipt => ({
  id: 'call_1',
  action: 'create_task',
  target_type: 'task',
  target_id: 'task-1',
  target_label: 'Ship the billing export',
  actor_id: 'user-1',
  actor_name: 'Alice',
  occurred_at: new Date().toISOString(),
  success: true,
  outcome: 'Task created',
  href: '/tasks?id=task-1',
  ...overrides,
});

const renderReceipts = (receipts: ActionReceipt[]) =>
  render(
    <BrowserRouter>
      <ActionReceipts receipts={receipts} />
    </BrowserRouter>
  );

describe('ActionReceipts', () => {
  it('renders nothing when there are no receipts', () => {
    const { container } = renderReceipts([]);
    expect(container.firstChild).toBeNull();
  });

  it('shows the action, target, actor, time, outcome, and item link', () => {
    renderReceipts([receipt()]);

    expect(screen.getByText('Created task')).toBeInTheDocument();
    expect(screen.getByText('Ship the billing export')).toBeInTheDocument();
    expect(screen.getByText('Alice')).toBeInTheDocument();
    expect(screen.getByText('Task created')).toBeInTheDocument();
    const link = screen.getByTestId('action-receipt-link');
    expect(link).toHaveTextContent('Open task');
    expect(link).toHaveAttribute('href', '/tasks?id=task-1');
  });

  it('links each workspace item type to its page', () => {
    renderReceipts([
      receipt({
        id: 'doc',
        action: 'create_document',
        target_type: 'document',
        href: '/wiki?id=doc-1',
      }),
      receipt({
        id: 'msg',
        action: 'send_message',
        target_type: 'message',
        href: '/chats?id=chat-9&message=msg-3',
      }),
      receipt({
        id: 'rem',
        action: 'create_reminder',
        target_type: 'reminder',
        href: '/chats?id=chat-9',
      }),
    ]);

    expect(screen.getByText('Created document').closest('article')).toHaveClass(
      'action-receipt--ok'
    );
    expect(screen.getByText('Open document')).toHaveAttribute('href', '/wiki?id=doc-1');
    expect(screen.getByText('Open message')).toHaveAttribute(
      'href',
      '/chats?id=chat-9&message=msg-3'
    );
    expect(screen.getByText('Open chat')).toHaveAttribute('href', '/chats?id=chat-9');
  });

  it('marks a failed write and omits the link when there is no item', () => {
    renderReceipts([
      receipt({
        success: false,
        outcome: 'Workspace access denied',
        href: '',
        target_id: '',
      }),
    ]);

    const card = screen.getByTestId('action-receipt');
    expect(card).toHaveClass('action-receipt--failed');
    expect(screen.getByText('Create task failed')).toBeInTheDocument();
    expect(screen.getByText('Workspace access denied')).toBeInTheDocument();
    expect(screen.queryByTestId('action-receipt-link')).not.toBeInTheDocument();
  });

  it('falls back to the raw action name for an unknown write', () => {
    renderReceipts([receipt({ action: 'archive_task' })]);

    expect(screen.getByText('archive_task')).toBeInTheDocument();
  });
});
