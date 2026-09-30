import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';

interface Failure {
  received: string | undefined;
  length: number;
}

function failure(assertion: () => void): Failure {
  try {
    assertion();
  } catch (error) {
    const message = Bun.stripANSI((error as Error).message);
    return { received: message.match(/^Received: (.*)$/m)?.[1], length: message.length };
  }
  throw new Error('The assertion passed');
}

describe('a failed assertion', () => {
  it('prints an element as its HTML, not its object graph', () => {
    render(
      <nav>
        <a href="/chats">Chats</a>
        <a href="/models">Models</a>
        <a href="/settings">Settings</a>
      </nav>
    );

    const { received, length } = failure(() =>
      expect(screen.getByRole('link', { name: 'Chats' })).toBeNull()
    );

    expect(length).toBeLessThan(1_000);
    expect(received).toBe('<a href="/chats">Chats</a>');
  });

  it('prints a text node as its text', () => {
    render(<p>Signed out</p>);

    const { received } = failure(() =>
      expect(screen.getByText('Signed out').firstChild).toBeNull()
    );

    expect(received).toBe('#text "Signed out"');
  });

  it('prints any other node as its type', () => {
    const { received } = failure(() => expect(document.createDocumentFragment()).toBeNull());

    expect(received).toBe('DocumentFragment');
  });

  it('prints only the start of a long element', () => {
    const text = 'Signed out. '.repeat(100);
    render(<p>{text}</p>);

    const { received } = failure(() => expect(screen.getByText(/Signed out/)).toBeNull());

    expect(received).toStartWith('<p>Signed out. Signed out.');
    expect(received).toEndWith('…');
    expect(received?.length).toBeLessThan(text.length);
  });
});
