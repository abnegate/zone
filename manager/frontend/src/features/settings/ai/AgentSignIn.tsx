import { Badge, type BadgeProps, Button } from '@zone/ui';
import { type ReactNode, useEffect, useId, useRef, useState } from 'react';
import { AgentRequestError } from '../../../api/AgentRequestError';
import { agentsApi, type StartRequest } from '../../../api/agents';
import { formatDate } from '../../projects/utils/formatters';
import { ClaudeSteps } from './ClaudeSteps';
import { DeviceSteps } from './DeviceSteps';
import { LoopbackSteps } from './LoopbackSteps';
import { onThisMachine } from './onThisMachine';
import type { Agent, AgentState, AgentStatus, ClaudeScope, SignInFlow } from './schemas';
import { SignOutDialog } from './SignOutDialog';
import type { AgentAccess, Attempt, SignInAction } from './types';
import { useExpired } from './useExpired';
import './AgentSignIn.css';

export const POLL_INTERVAL = 3000;
export const POLL_INTERVAL_LIMIT = 30000;

type Awaiting = 'device' | 'browser';
type Outcome = 'waiting' | 'finished' | 'failed';

const names: Record<Agent, string> = { claude: 'Claude Code', codex: 'Codex' };
const accounts: Record<Agent, string> = { claude: 'Claude', codex: 'ChatGPT' };

const states: Record<AgentState, { label: string; tint: BadgeProps['variant'] }> = {
  signed_in: { label: 'Signed in', tint: 'success' },
  signed_out: { label: 'Not signed in', tint: 'neutral' },
  pending: { label: 'Signing in', tint: 'info' },
  expired: { label: 'Sign-in expired', tint: 'warning' },
};

type FocusTarget = 'entry' | 'status';

interface AgentSignInProps {
  organizationId: string;
  agent: Agent;
  access: AgentAccess;
  unsaved: boolean;
  heading: 'h3' | 'h4';
  status: AgentStatus | undefined;
  attempt: Attempt | undefined;
  loadError: string | null;
  onStatusChange: (status: AgentStatus) => void;
  onAttemptChange: (agent: Agent, attempt: Attempt | null) => void;
}

function reasonOf(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

function startRequest(agent: Agent, scope?: ClaudeScope, flow?: SignInFlow): StartRequest {
  const remote = agent === 'claude' && !onThisMachine(window.location.hostname);
  const chosen = flow ?? (remote ? 'paste' : undefined);
  return { ...(scope && { scope }), ...(chosen && { flow: chosen }) };
}

function deviceOutcome(status: AgentStatus): Outcome {
  return status.state === 'pending' ? 'waiting' : 'finished';
}

function browserOutcome(status: AgentStatus): Outcome {
  if (status.state === 'signed_in' && status.source === 'zone') return 'finished';
  return status.error ? 'failed' : 'waiting';
}

function signedInDetail(status: AgentStatus, agent: Agent, lapsed: boolean): string {
  if (status.source === 'host') {
    const plan = status.label ? ` (${status.label})` : '';
    return `Using this server's own ${names[agent]} sign-in${plan}.`;
  }
  const expiry = status.expires_at && !lapsed ? formatDate(status.expires_at) : null;
  const parts = [status.label, expiry && `Expires ${expiry}`].filter(Boolean);
  return parts.length > 0 ? parts.join(' · ') : 'Signed in for this organization.';
}

export function AgentSignIn({
  organizationId,
  agent,
  access,
  unsaved,
  heading,
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
  const [confirming, setConfirming] = useState(false);
  const [focus, setFocus] = useState<FocusTarget | null>(null);
  const shown = useRef(organizationId);
  const runs = useRef(0);
  const section = useRef<HTMLElement>(null);
  const statusLine = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const deviceCode = useRef<HTMLElement>(null);
  const link = useRef<HTMLAnchorElement>(null);
  const latest = useRef(attempt);

  useEffect(() => {
    shown.current = organizationId;
  }, [organizationId]);

  useEffect(() => {
    latest.current = attempt;
  }, [attempt]);

  useEffect(() => {
    if (focus === null) return;
    setFocus(null);
    const active = document.activeElement;
    const away =
      active?.isConnected && active !== document.body && !section.current?.contains(active);
    if (away) return;
    const target =
      focus === 'entry'
        ? (field.current ?? deviceCode.current ?? link.current)
        : statusLine.current;
    target?.focus();
  }, [focus]);

  const authorization = attempt?.login.agent === 'claude' ? attempt.login : null;
  const expired = useExpired(authorization?.expires_at ?? null);
  const usable = authorization !== null && !attempt?.spent && !expired;
  const returning = usable && authorization.flow === 'loopback';
  const prompt = attempt?.login.agent === 'codex' ? attempt.login : null;
  const pending = status?.state === 'pending';
  const waiting = prompt !== null || pending;
  const awaiting: Awaiting | null = waiting ? 'device' : returning ? 'browser' : null;
  const watched = awaiting === 'browser' ? authorization?.attempt : undefined;
  const device = pending ? (status?.pending ?? null) : prompt;
  const codeExpired = useExpired(device?.expires_at ?? null);
  const lapsed = useExpired(status?.state === 'signed_in' ? status.expires_at : null);

  const outdated = expired || codeExpired;
  const wasOutdated = useRef(outdated);

  useEffect(() => {
    if (outdated && !wasOutdated.current) setFocus('status');
    wasOutdated.current = outdated;
  }, [outdated]);

  useEffect(() => {
    if (awaiting === null) return;
    let cancelled = false;
    let failures = 0;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await agentsApi.get(organizationId, agent, watched);
        if (cancelled) return;
        failures = 0;
        setFailure(null);
        onStatusChange(next);
        const outcome = awaiting === 'device' ? deviceOutcome(next) : browserOutcome(next);
        if (outcome === 'waiting') {
          timer = setTimeout(poll, POLL_INTERVAL);
          return;
        }
        if (outcome === 'failed') {
          setFailure(next.error);
          const current = latest.current;
          onAttemptChange(agent, current ? { ...current, spent: true } : null);
        } else {
          onAttemptChange(agent, null);
        }
        setFocus('status');
      } catch (reason) {
        if (cancelled) return;
        setFailure(reasonOf(reason));
        if (reason instanceof AgentRequestError && !reason.retryable) return;
        failures += 1;
        timer = setTimeout(poll, Math.min(POLL_INTERVAL * 2 ** failures, POLL_INTERVAL_LIMIT));
      }
    };
    timer = setTimeout(poll, POLL_INTERVAL);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [awaiting, watched, organizationId, agent, onStatusChange, onAttemptChange]);

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

  const start = (action: SignInAction, scope?: ClaudeScope, flow?: SignInFlow) =>
    perform(action, async (current) => {
      const login = await agentsApi.start(organizationId, agent, startRequest(agent, scope, flow));
      if (!current()) return;
      onAttemptChange(agent, { login, scope, flow, spent: false });
      setCode('');
      setFocus('entry');
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
        setFocus('status');
      },
      (reason) => {
        const kind = reason instanceof AgentRequestError ? reason.kind : undefined;
        if (kind === 'invalid_code') {
          setCodeError(reasonOf(reason));
          setFocus('entry');
          return;
        }
        setFailure(reasonOf(reason));
        if (kind === 'start_again') {
          onAttemptChange(agent, { ...attempt, spent: true });
          setFocus('status');
        }
      }
    );
  };

  const signOut = () =>
    perform('signOut', async (current) => {
      await agentsApi.signOut(organizationId, agent);
      if (!current()) return;
      onAttemptChange(agent, null);
      const next = await agentsApi.get(organizationId, agent);
      if (!current()) return;
      onStatusChange(next);
      setFocus('status');
    });

  const confirmSignOut = () => {
    setConfirming(false);
    void signOut();
  };

  const cancel = () =>
    perform('cancel', async (current) => {
      await agentsApi.cancel(organizationId, agent);
      if (!current()) return;
      onAttemptChange(agent, null);
      setCode('');
      setFocus('status');
    });

  const name = names[agent];
  const account = accounts[agent];
  const signingIn = usable || waiting;
  const manage = access === 'manage';
  const manageable = manage && status !== undefined;
  const offerSignIn =
    manageable &&
    !authorization &&
    !waiting &&
    (status.state !== 'signed_in' || status.source === 'host');
  const offerSignOut = manageable && !authorization && !waiting && status.source === 'zone';
  const quiet = signingIn || status?.state === 'signed_in';
  const error = failure ?? (status ? (quiet ? null : status.error) : loadError);
  const state = signingIn ? 'pending' : (status?.state ?? 'signed_out');
  const badge = states[state === 'pending' && !manage ? 'signed_out' : state];

  let detail: string | null = null;
  if (manage && returning) {
    detail = 'Approve on claude.com; Zone finishes the sign-in automatically.';
  } else if (manage && usable) {
    detail = 'Waiting for the code from claude.com.';
  } else if (status?.state === 'signed_in') {
    detail = signedInDetail(status, agent, lapsed);
  } else if (status && !manage) {
    detail = access === 'view' ? 'Ask an organization admin to sign in.' : null;
  } else if (authorization && expired) {
    detail = 'The link from claude.com expired. Start again to get a new one.';
  } else if (authorization) {
    detail = 'Start again to get a new link from claude.com.';
  } else if (waiting && codeExpired) {
    detail = 'The one-time code expired. Cancel, then sign in again.';
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
            onClick={() => setConfirming(true)}
            loading={busy === 'signOut'}
            disabled={busy !== null}
          >
            Sign out
          </Button>
        )}
      </div>
    );
  }

  const Heading = heading;

  return (
    <section ref={section} className="agent-sign-in" aria-labelledby={headingId}>
      <Heading id={headingId} className="settings-eyebrow">
        {name} sign-in
      </Heading>

      {status ? (
        <div className="agent-sign-in-status">
          <div ref={statusLine} className="agent-sign-in-state" role="status" tabIndex={-1}>
            <Badge variant={badge.tint}>{badge.label}</Badge>
            <p className="agent-sign-in-detail">{detail}</p>
            {status.state === 'signed_in' && unsaved && (
              <p className="form-hint agent-sign-in-note">Save Changes to use this provider.</p>
            )}
          </div>
          {actions}
        </div>
      ) : (
        !loadError && <p className="agent-sign-in-detail">Checking sign-in…</p>
      )}

      {manageable &&
        authorization &&
        attempt &&
        (authorization.flow === 'loopback' ? (
          <LoopbackSteps
            url={authorization.authorize_url}
            full={attempt.scope === 'full'}
            expiresAt={authorization.expires_at}
            usable={usable}
            busy={busy}
            entry={link}
            onRestart={() => void start('restart', attempt.scope, attempt.flow)}
            onFullAccess={() => void start('full', 'full', attempt.flow)}
            onPaste={() => void start('paste', attempt.scope, 'paste')}
            onCancel={() => void cancel()}
          />
        ) : (
          <ClaudeSteps
            url={authorization.authorize_url}
            full={attempt.scope === 'full'}
            expiresAt={authorization.expires_at}
            usable={usable}
            code={code}
            codeError={codeError}
            busy={busy}
            entry={field}
            onCodeChange={setCode}
            onSubmit={submit}
            onRestart={() => void start('restart', attempt.scope, attempt.flow)}
            onFullAccess={() => void start('full', 'full', attempt.flow)}
            onCancel={() => void cancel()}
          />
        ))}

      {manageable && waiting && (
        <DeviceSteps
          prompt={codeExpired ? null : device}
          account={account}
          busy={busy}
          entry={deviceCode}
          onCancel={() => void signOut()}
        />
      )}

      <SignOutDialog
        open={confirming}
        name={name}
        onConfirm={confirmSignOut}
        onClose={() => setConfirming(false)}
      />

      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
    </section>
  );
}
