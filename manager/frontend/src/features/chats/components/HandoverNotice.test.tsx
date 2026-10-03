import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { Handover } from '../types';
import { HandoverNotice } from './HandoverNotice';

const NOW = Date.parse('2026-09-23T04:00:00Z');
const RESETS_AT = new Date(NOW + (2 * 60 + 10) * 60_000).toISOString();

const handover = (overrides: Partial<Handover> = {}): Handover => ({
  kind: 'handover',
  from: 'a@example.com',
  to: 'b@example.com',
  agent: 'claude',
  reason: 'limit',
  resets_at: RESETS_AT,
  carried: true,
  at: 0,
  ...overrides,
});

describe('HandoverNotice', () => {
  it('says which account took over and when the spent one resets', () => {
    render(<HandoverNotice handover={handover()} agentChanged={false} now={NOW} />);

    const note = screen.getByRole('note');
    expect(note).toHaveTextContent(
      'Switched to b@example.com — a@example.com reached its usage limit; resets in 2h 10m'
    );
    expect(note.querySelector('strong')).toHaveTextContent(/^b@example\.com$/);
  });

  it('says the account ran out of credits', () => {
    render(
      <HandoverNotice handover={handover({ reason: 'credits' })} agentChanged={false} now={NOW} />
    );

    expect(screen.getByRole('note')).toHaveTextContent(
      'Switched to b@example.com — a@example.com ran out of credits; resets in 2h 10m'
    );
  });

  it('says a signed-out account was signed out, with no reset to wait for', () => {
    render(
      <HandoverNotice
        handover={handover({ reason: 'signed_out' })}
        agentChanged={false}
        now={NOW}
      />
    );

    const note = screen.getByRole('note');
    expect(note).toHaveTextContent('Switched to b@example.com — a@example.com was signed out');
    expect(note).not.toHaveTextContent('resets');
  });

  it('speaks of the previous account when the switch names none', () => {
    render(
      <HandoverNotice handover={handover({ from: undefined })} agentChanged={false} now={NOW} />
    );

    expect(screen.getByRole('note')).toHaveTextContent(
      'Switched to b@example.com — the previous account reached its usage limit; resets in 2h 10m'
    );
  });

  it('names the agent the turn moved to when it changed', () => {
    render(<HandoverNotice handover={handover({ agent: 'codex' })} agentChanged now={NOW} />);

    const note = screen.getByRole('note');
    expect(note).toHaveTextContent(
      'Switched to Codex · b@example.com — a@example.com reached its usage limit; resets in 2h 10m'
    );
    expect(note.querySelector('strong')).toHaveTextContent('Codex · b@example.com');
  });

  it('leaves the agent out when it did not change', () => {
    render(<HandoverNotice handover={handover({ agent: 'codex' })} agentChanged={false} />);

    expect(screen.getByRole('note')).not.toHaveTextContent('Codex');
  });
});
