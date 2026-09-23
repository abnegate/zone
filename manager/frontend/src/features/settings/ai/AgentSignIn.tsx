import { Badge, type BadgeProps, Button } from '@zone/ui';
import { type ReactNode, useEffect, useId, useRef, useState } from 'react';
import { AgentRequestError } from '../../../api/AgentRequestError';
import { agentsApi } from '../../../api/agents';
import { ClaudeSteps } from './ClaudeSteps';
import { DeviceSteps } from './DeviceSteps';
import type { Agent, AgentState, AgentStatus, ClaudeScope } from './schemas';
import type { Attempt, SignInAction } from './types';
import { useExpired } from './useExpired';
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

interface AgentSignInProps {
  organizationId: string;
  agent: Agent;
  canManage: boolean;
  status: AgentStatus | undefined;
  attempt: Attempt | undefined;
  loadError: string | null;
  onStatusChange: (status: AgentStatus) => void;
  onAttemptChange: (agent: Agent, attempt: Attempt | null) => void;
}

function reasonOf(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

function formatDate(value: string): string | null {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric' });
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
  attempt,
  loadError,
  onStatusChange,
  onAttemptChange,
}: AgentSignInProps) {
  const headingId = useId();
  const [code, setCode] = useState('');
  const [codeError, setCodeError] = useState<string | null>(null);
  const [busy, setBusy] = useState<SignInAction | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const shown = useRef(organizationId);
  const runs = useRef(0);

  useEffect(() => {
    shown.current = organizationId;
  }, [organizationId]);

  const authorization = attempt?.login.agent === 'claude' ? attempt.login : null;
  const expired = useExpired(authorization?.expires_at ?? null);
  const usable = authorization !== null && !attempt?.spent && !expired;
  const prompt = attempt?.login.agent === 'codex' ? attempt.login : null;
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
          onAttemptChange(agent, null);
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
  }, [waiting, organizationId, agent, onStatusChange, onAttemptChange]);

  const perform = async (
    action: SignInAction,
    work: (current: () => boolean) => Promise<void>,
    fail: (reason: unknown) => void = (reason) => setFailure(reasonOf(reason))
  ): Promise<void> => {
    const run = ++runs.current;
    const current = () => shown.current === organizationId;
    setBusy(action);
    setFailure(null);
    setCodeError(null);
    try {
      await work(current);
    } catch (reason) {
      if (current()) fail(reason);
    } finally {
      if (runs.current === run) setBusy(null);
    }
  };

  const start = (action: SignInAction, scope?: ClaudeScope) =>
    perform(action, async (current) => {
      const login = await agentsApi.start(organizationId, agent, scope);
      if (!current()) return;
      onAttemptChange(agent, { login, scope, spent: false });
      setCode('');
    });

  const submit = () => {
    const value = code.trim();
    if (!value || busy !== null || !attempt) return;
    void perform(
      'submit',
      async (current) => {
        const next = await agentsApi.submitCode(organizationId, value);
        if (!current()) return;
        onAttemptChange(agent, null);
        setCode('');
        onStatusChange(next);
      },
      (reason) => {
        const kind = reason instanceof AgentRequestError ? reason.kind : undefined;
        if (kind === 'invalid_code') {
          setCodeError(reasonOf(reason));
          return;
        }
        setFailure(reasonOf(reason));
        if (kind === 'start_again') onAttemptChange(agent, { ...attempt, spent: true });
      }
    );
  };

  const signOut = () =>
    perform('signOut', async (current) => {
      await agentsApi.signOut(organizationId, agent);
      if (!current()) return;
      onAttemptChange(agent, null);
      const next = await agentsApi.get(organizationId, agent);
      if (current()) onStatusChange(next);
    });

  const abandon = () => {
    onAttemptChange(agent, null);
    setCode('');
    setCodeError(null);
    setFailure(null);
  };

  const name = names[agent];
  const account = accounts[agent];
  const signingIn = usable || waiting;
  const manageable = canManage && status !== undefined;
  const offerSignIn =
    manageable &&
    !authorization &&
    !waiting &&
    (status.state !== 'signed_in' || status.source === 'host');
  const offerSignOut = manageable && !authorization && !waiting && status.source === 'zone';
  const error = failure ?? (status ? (signingIn ? null : status.error) : loadError);
  const badge = states[signingIn ? 'pending' : (status?.state ?? 'signed_out')];

  let detail: string | null = null;
  if (status?.state === 'signed_in') {
    detail = signedInDetail(status, agent);
  } else if (status && !canManage) {
    detail = 'Ask an organization admin to sign in.';
  } else if (usable) {
    detail = 'Waiting for the code from claude.com.';
  } else if (authorization && expired) {
    detail = 'The link from claude.com expired. Start again to get a new one.';
  } else if (authorization) {
    detail = 'Start again to get a new link from claude.com.';
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
            onClick={() => void start('start')}
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

      {manageable && authorization && attempt && (
        <ClaudeSteps
          url={authorization.authorize_url}
          full={attempt.scope === 'full'}
          expiresAt={authorization.expires_at}
          usable={usable}
          code={code}
          codeError={codeError}
          busy={busy}
          onCodeChange={setCode}
          onSubmit={submit}
          onRestart={() => void start('restart', attempt.scope)}
          onFullAccess={() => void start('full', 'full')}
          onCancel={abandon}
        />
      )}

      {manageable && waiting && (
        <DeviceSteps
          prompt={device}
          account={account}
          busy={busy}
          onCancel={() => void signOut()}
        />
      )}

      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
    </section>
  );
}
