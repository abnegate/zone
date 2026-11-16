import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { AGENT_POLICY_LINK } from '../../enterprise';
import { AGENT_POLICY_PATH, ChatError } from './ChatError';

function renderError(message: string, denial: Parameters<typeof ChatError>[0]['denial']) {
  return render(
    <MemoryRouter>
      <ChatError message={message} denial={denial} />
    </MemoryRouter>
  );
}

describe('ChatError', () => {
  it('shows a plain failure as it came', () => {
    renderError('Rate limit exceeded', null);

    expect(screen.getByRole('alert')).toHaveTextContent('Rate limit exceeded');
    expect(screen.queryByRole('link')).toBeNull();
  });

  it("shows the server's wording for a denied turn and links to the workspace's agent policy", () => {
    const worded = "Your organization's policy doesn't allow OpenAI API in this workspace.";
    renderError(worded, 'policy_provider_denied');

    const alert = screen.getByRole('alert');
    expect(alert).toHaveClass('chats-error');
    expect(alert).toHaveClass('chats-denial');
    expect(alert).toHaveTextContent(worded);
    expect(alert).not.toHaveTextContent('this provider');
    expect(screen.getByRole('link', { name: AGENT_POLICY_LINK })).toHaveAttribute(
      'href',
      AGENT_POLICY_PATH
    );
    expect(AGENT_POLICY_PATH).toBe('/settings/workspace/agent-policy');
  });

  it('words a denied turn from its code when the server sent no message', () => {
    renderError('  ', 'policy_model_denied');

    expect(screen.getByRole('alert')).toHaveTextContent(
      "Your organization's policy doesn't allow this model in this workspace. Choose an allowed model."
    );
  });
});
