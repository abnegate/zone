import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { MessageMetadata } from '../types';
import { AutomationBadge } from './AutomationBadge';

const REMINDER_ID = 'a5e0f1c2-0000-4000-8000-000000000001';

describe('AutomationBadge', () => {
  it('marks a message a reminder wrote as the reminder, not the person', () => {
    render(<AutomationBadge metadata={{ source: 'reminder', reminder_id: REMINDER_ID }} />);

    const badge = screen.getByTestId('automation-badge');
    expect(badge).toHaveTextContent('Reminder');
    expect(badge).toHaveTextContent('Sent by a reminder on its schedule, not typed in this chat.');
    expect(badge).toHaveClass('ui-badge');
    expect(badge).toHaveClass('ui-badge-neutral');
  });

  it('names the reminder that fired in the tooltip', () => {
    render(<AutomationBadge metadata={{ source: 'reminder', reminder_id: REMINDER_ID }} />);

    expect(screen.getByTestId('automation-badge')).toHaveAttribute(
      'title',
      `Sent by a reminder on its schedule, not typed in this chat. Reminder ${REMINDER_ID}.`
    );
  });

  it('leaves the tooltip as the bare explanation when no reminder is named', () => {
    render(<AutomationBadge metadata={{ source: 'reminder', reminder_id: '  ' }} />);

    expect(screen.getByTestId('automation-badge')).toHaveAttribute(
      'title',
      'Sent by a reminder on its schedule, not typed in this chat.'
    );
  });

  it('ignores a reminder id that is not a string', () => {
    const metadata = { source: 'reminder', reminder_id: 7 } as unknown as MessageMetadata;
    render(<AutomationBadge metadata={metadata} />);

    expect(screen.getByTestId('automation-badge')).toHaveAttribute(
      'title',
      'Sent by a reminder on its schedule, not typed in this chat.'
    );
  });

  it('draws its icon for sighted readers only', () => {
    render(<AutomationBadge metadata={{ source: 'reminder' }} />);

    const icon = screen.getByTestId('automation-badge').querySelector('svg');
    expect(icon).toHaveAttribute('aria-hidden', 'true');
    expect(icon?.querySelector('path')?.getAttribute('d')).toBeTruthy();
  });

  it('labels an auto project from the same table', () => {
    render(<AutomationBadge metadata={{ source: 'auto_project' }} />);

    expect(screen.getByTestId('automation-badge')).toHaveTextContent('Auto project');
  });

  it('labels what auto-fix wrote while working on a pull request', () => {
    render(<AutomationBadge metadata={{ source: 'pull_request_fix' }} />);

    const badge = screen.getByTestId('automation-badge');
    expect(badge).toHaveTextContent('Auto-fix');
    expect(badge).toHaveTextContent('Written by auto-fix as it worked on a pull request');
  });

  it('renders nothing for a message a person typed', () => {
    const { container } = render(<AutomationBadge metadata={{ attachments: [] }} />);

    expect(container.firstChild).toBeNull();
  });

  it('renders nothing without metadata', () => {
    const { container } = render(<AutomationBadge metadata={null} />);

    expect(container.firstChild).toBeNull();
  });

  it('renders nothing for a source it does not know', () => {
    for (const source of ['workspace_tool', 'toString', 'constructor', '', 7]) {
      const metadata = { source } as unknown as MessageMetadata;
      const { container, unmount } = render(<AutomationBadge metadata={metadata} />);

      expect(container.firstChild).toBeNull();
      unmount();
    }
  });
});
