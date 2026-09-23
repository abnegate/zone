import { Badge, type BadgeProps, Button, buttonVariants } from '@zone/ui';
import { type KeyboardEvent, type ReactNode, useEffect, useId, useState } from 'react';
import { agentsApi } from '../../../api/agents';
import type { Agent, AgentState, AgentStatus, ClaudeScope, DevicePrompt } from './schemas';
import './AgentSignIn.css';

export const POLL_INTERVAL = 3000;

const names: Record<Agent, string> = { claude: 'Claude Code', codex: 'Codex' };
const accounts: Record<Agent, string> = { claude: 'Claude', codex: 'ChatGPT' };

const states: Record<AgentState, { label: string; tint: BadgeProps['variant'] }> = {
  signed_in: { label: 'Signed in', tint: 'success' },
  signed_out: { label: 'Not signed in', tint: 'neutral' },
  pending: { label: 'Signing in', tint: 'info' },
  expired: { label: 'Sign-in expired', tint: 'warning' },
};

type Action = 'start' | 'full' | 'submit' | 'signOut';

interface Authorization {
  url: string;
  scope: ClaudeScope | undefined;
}

interface AgentSignInProps {
  organizationId: string;
  agent: Agent;
  canManage: boolean;
  status: AgentStatus | undefined;
  loadError: string | null;
  onStatusChange: (status: AgentStatus) => void;
}

function reasonOf(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

function formatDate(value: string): string | null {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric' });
}

function formatTime(value: string): string | null {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
}

function signedInDetail(status: AgentStatus, agent: Agent): string {
  if (status.source === 'host') {
    const plan = status.label ? ` (${status.label})` : '';
    return `Using this server's own ${names[agent]} sign-in${plan}.`;
  }
  const expiry = status.expires_at ? formatDate(status.expires_at) : null;
  const parts = [status.label, expiry && `Expires ${expiry}`].filter(Boolean);
  return parts.length > 0 ? parts.join(' · ') : 'Signed in for this organization.';
}

export function AgentSignIn({
  organizationId,
  agent,
  canManage,
  status,
  loadError,
  onStatusChange,
}: AgentSignInProps) {
  const headingId = useId();
  const codeId = useId();
  const [authorization, setAuthorization] = useState<Authorization | null>(null);
  const [prompt, setPrompt] = useState<DevicePrompt | null>(null);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState<Action | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [rejected, setRejected] = useState(false);

  const pending = status?.state === 'pending';
  const waiting = prompt !== null || pending;
  const device = prompt ?? (pending ? (status?.pending ?? null) : null);

  useEffect(() => {
    if (!waiting) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await agentsApi.get(organizationId, agent);
        if (cancelled) return;
        setFailure(null);
        onStatusChange(next);
        if (next.state === 'pending') {
          timer = setTimeout(poll, POLL_INTERVAL);
        } else {
          setPrompt(null);
        }
      } catch (reason) {
        if (cancelled) return;
        setFailure(reasonOf(reason));
        timer = setTimeout(poll, POLL_INTERVAL);
      }
    };
    timer = setTimeout(poll, POLL_INTERVAL);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [waiting, organizationId, agent, onStatusChange]);

  const perform = async (action: Action, work: () => Promise<void>): Promise<boolean> => {
    setBusy(action);
    setFailure(null);
    try {
      await work();
      return true;
    } catch (reason) {
      setFailure(reasonOf(reason));
      return false;
    } finally {
      setBusy(null);
    }
  };

  const start = (scope?: ClaudeScope) =>
    perform(scope === 'full' ? 'full' : 'start', async () => {
      const login = await agentsApi.start(organizationId, agent, scope);
      if (login.agent === 'claude') {
        setAuthorization({ url: login.authorize_url, scope });
        setCode('');
        setRejected(false);
      } else {
        setPrompt({
          verification_url: login.verification_url,
          user_code: login.user_code,
          expires_at: login.expires_at,
        });
      }
    });

  const submit = async () => {
    const value = code.trim();
    if (!value || busy) return;
    const exchanged = await perform('submit', async () => {
      const next = await agentsApi.submitCode(organizationId, value);
      setAuthorization(null);
      setCode('');
      onStatusChange(next);
    });
    setRejected(!exchanged);
  };

  const signOut = () =>
    perform('signOut', async () => {
      await agentsApi.signOut(organizationId, agent);
      setPrompt(null);
      setAuthorization(null);
      onStatusChange(await agentsApi.get(organizationId, agent));
    });

  const abandon = () => {
    setAuthorization(null);
    setCode('');
    setFailure(null);
    setRejected(false);
  };

  const submitOnEnter = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    void submit();
  };

  const name = names[agent];
  const account = accounts[agent];
  const signingIn = authorization !== null || waiting;
  const manageable = canManage && status !== undefined;
  const offerSignIn =
    manageable && !signingIn && (status.state !== 'signed_in' || status.source === 'host');
  const offerSignOut = manageable && !signingIn && status.source === 'zone';
  const expiresAt = device ? formatTime(device.expires_at) : null;
  const error = failure ?? (status ? (signingIn ? null : status.error) : loadError);
  const badge = states[signingIn ? 'pending' : (status?.state ?? 'signed_out')];

  let detail: string | null = null;
  if (status?.state === 'signed_in') {
    detail = signedInDetail(status, agent);
  } else if (status && !canManage) {
    detail = 'Ask an organization admin to sign in.';
  } else if (authorization) {
    detail = 'Waiting for the code from claude.com.';
  } else if (waiting) {
    detail = 'Waiting for you to finish signing in.';
  } else if (status?.state === 'expired') {
    detail = `Sign in again to keep using ${name}.`;
  } else if (status) {
    detail = `Sign in with a ${account} subscription to use ${name} in this organization.`;
  }

  let actions: ReactNode = null;
  if (offerSignIn || offerSignOut) {
    actions = (
      <div className="agent-sign-in-actions">
        {offerSignIn && (
          <Button
            size="sm"
            onClick={() => void start()}
            loading={busy === 'start'}
            disabled={busy !== null}
          >
            Sign in with {account}
          </Button>
        )}
        {offerSignOut && (
          <Button
            size="sm"
            variant="secondary"
            onClick={() => void signOut()}
            loading={busy === 'signOut'}
            disabled={busy !== null}
          >
            Sign out
          </Button>
        )}
      </div>
    );
  }

  return (
    <section className="agent-sign-in" aria-labelledby={headingId}>
      <h4 id={headingId} className="settings-eyebrow">
        {name} sign-in
      </h4>

      {status ? (
        <div className="agent-sign-in-status">
          <Badge variant={badge.tint}>{badge.label}</Badge>
          <p className="agent-sign-in-detail">{detail}</p>
          {actions}
        </div>
      ) : (
        !loadError && <p className="agent-sign-in-detail">Checking sign-in…</p>
      )}

      {manageable && authorization && (
        <ol className="agent-sign-in-steps">
          <li>
            <div className="agent-sign-in-step">
              <span>Open claude.com, sign in, and approve access.</span>
              <a
                className={buttonVariants({ variant: 'secondary', size: 'sm' })}
                href={authorization.url}
                target="_blank"
                rel="noopener noreferrer"
              >
                Open claude.com
              </a>
            </div>
            {authorization.scope === 'full' && (
              <p className="form-hint">This link asks for full access to your Claude account.</p>
            )}
          </li>
          <li>
            <div className="form-group">
              <label htmlFor={codeId}>Code from claude.com</label>
              <input
                id={codeId}
                type="text"
                className="form-input"
                value={code}
                onChange={(event) => setCode(event.target.value)}
                onKeyDown={submitOnEnter}
                placeholder="code#state, or the address of the page showing it"
                autoComplete="off"
                autoCapitalize="off"
                spellCheck={false}
              />
              <p className="form-hint">
                Paste the code claude.com shows. If claude.com refuses the request, try again with
                full access.
              </p>
            </div>
            <div className="agent-sign-in-buttons">
              <Button
                size="sm"
                onClick={() => void submit()}
                loading={busy === 'submit'}
                disabled={busy !== null || !code.trim()}
              >
                Submit code
              </Button>
              <Button
                size="sm"
                variant={rejected ? 'secondary' : 'ghost'}
                onClick={() => void start('full')}
                loading={busy === 'full'}
                disabled={busy !== null}
              >
                Try again with full access
              </Button>
              <Button size="sm" variant="ghost" onClick={abandon} disabled={busy !== null}>
                Cancel
              </Button>
            </div>
          </li>
        </ol>
      )}

      {manageable && waiting && (
        <>
          {device && (
            <ol className="agent-sign-in-steps">
              <li>
                <div className="agent-sign-in-step">
                  <span>
                    Open{' '}
                    <a
                      className="agent-sign-in-link"
                      href={device.verification_url}
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      {device.verification_url.replace(/^https?:\/\//, '')}
                    </a>{' '}
                    and sign in with {account}.
                  </span>
                </div>
              </li>
              {device.user_code && (
                <li>
                  <div className="agent-sign-in-step">
                    <span>Enter this one-time code:</span>
                    <code className="agent-sign-in-code">{device.user_code}</code>
                  </div>
                  <p className="form-hint">
                    {expiresAt && `Expires at ${expiresAt}. `}
                    Only enter it if you started this sign-in here. If a website or someone else
                    gave you this code, cancel.
                  </p>
                </li>
              )}
            </ol>
          )}
          <div className="agent-sign-in-buttons">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void signOut()}
              loading={busy === 'signOut'}
              disabled={busy !== null}
            >
              Cancel
            </Button>
          </div>
        </>
      )}

      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
    </section>
  );
}
