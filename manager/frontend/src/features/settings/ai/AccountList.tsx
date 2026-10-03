import { Badge, type BadgeProps, Button } from '@zone/ui';
import { formatDate } from '../../projects/utils/formatters';
import type { AgentAccount, LoginState } from './schemas';
import { UsageBar } from './UsageBar';
import { accountLabel, exhaustedUntil, moment } from './usage';

const states: Record<LoginState, { label: string; tint: BadgeProps['variant'] }> = {
  signed_in: { label: 'Signed in', tint: 'success' },
  signed_out: { label: 'Signed out', tint: 'neutral' },
  pending: { label: 'Signing in', tint: 'info' },
  expired: { label: 'Sign-in expired', tint: 'warning' },
};

const exhausted = { label: 'Limit reached', tint: 'destructive' } as const;

interface AccountListProps {
  name: string;
  accounts: AgentAccount[];
  now: number;
  manage: boolean;
  busy: boolean;
  leaving: string | null;
  onSignOut: (account: AgentAccount) => void;
}

function metaOf(account: AgentAccount, now: number): string {
  const until = exhaustedUntil(account, now);
  const expires =
    account.expires_at !== null && Date.parse(account.expires_at) > now ? account.expires_at : null;
  return [
    until && `Exhausted until ${moment(until, now)}`,
    expires && `Expires ${formatDate(expires)}`,
    account.last_used_at && `Last used ${moment(account.last_used_at, now)}`,
    account.usage && `Usage checked ${moment(account.usage.fetched_at, now)}`,
  ]
    .filter(Boolean)
    .join(' · ');
}

export function AccountList({
  name,
  accounts,
  now,
  manage,
  busy,
  leaving,
  onSignOut,
}: AccountListProps) {
  return (
    <ul className="agent-accounts" aria-label={`${name} accounts`}>
      {accounts.map((account) => {
        const label = accountLabel(account);
        const plan = account.label !== null ? account.plan : null;
        const badge =
          account.state === 'signed_in' && exhaustedUntil(account, now)
            ? exhausted
            : states[account.state];
        const meta = metaOf(account, now);
        const windows = account.usage?.windows ?? [];
        return (
          <li key={account.id} className="agent-account">
            <div className="agent-account-head">
              <span className="agent-account-label">{label}</span>
              {plan && <span className="agent-account-plan">{plan}</span>}
              <Badge variant={badge.tint}>{badge.label}</Badge>
              {manage && (
                <Button
                  size="sm"
                  variant="secondary"
                  className="agent-account-sign-out"
                  aria-label={`Sign out ${label}`}
                  onClick={() => onSignOut(account)}
                  loading={leaving === account.id}
                  disabled={busy}
                >
                  Sign out
                </Button>
              )}
            </div>
            {account.usage === null ? (
              <p className="agent-account-note">Usage unavailable</p>
            ) : windows.length === 0 ? (
              <p className="agent-account-note">No limits reported</p>
            ) : (
              <div className="usage-bars">
                {windows.map((window) => (
                  <UsageBar key={window.name} window={window} now={now} />
                ))}
              </div>
            )}
            {meta && <p className="agent-account-meta">{meta}</p>}
          </li>
        );
      })}
    </ul>
  );
}
