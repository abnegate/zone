import { Button } from '@zone/ui';
import { useEffect, useState } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { agentsApi } from '../../../api/agents';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { AuthCard, AuthStatus, CheckIcon } from '../../auth/components';
import type { AgentStatus } from './schemas';

const SETTINGS = '/org-settings';
const RECEIPT = 'receipt';
const ORGANIZATION = 'organization';
const TITLE = 'Claude Code sign-in';

type Outcome =
  | { state: 'finishing' }
  | { state: 'signed-in' }
  | { state: 'failed'; reason: string };

// One request per receipt while it is in flight: StrictMode mounts the page twice, and a second
// request would find the single-use receipt spent.
const inFlight = new Map<string, Promise<AgentStatus>>();

function redeemOnce(organization: string, receipt: string): Promise<AgentStatus> {
  const pending = inFlight.get(receipt);
  if (pending) return pending;
  const request = agentsApi.redeem(organization, receipt).finally(() => inFlight.delete(receipt));
  inFlight.set(receipt, request);
  return request;
}

/** Where Zone's callback sends a browser claude.com returned a sign-in to, to finish it. */
export default function AgentSignInPage() {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const [search] = useSearchParams();
  const { organizations, setCurrentOrganization } = useWorkspace();
  const [returned] = useState(() => ({
    receipt: search.get(RECEIPT),
    organization: search.get(ORGANIZATION),
  }));
  const [outcome, setOutcome] = useState<Outcome>({ state: 'finishing' });

  useEffect(() => {
    navigate(pathname, { replace: true });
  }, [navigate, pathname]);

  useEffect(() => {
    const { receipt, organization } = returned;
    if (!receipt || !organization) return;
    let current = true;
    redeemOnce(organization, receipt).then(
      () => {
        if (current) setOutcome({ state: 'signed-in' });
      },
      (reason: unknown) => {
        if (current) {
          setOutcome({
            state: 'failed',
            reason: reason instanceof Error ? reason.message : String(reason),
          });
        }
      }
    );
    return () => {
      current = false;
    };
  }, [returned]);

  const settle = () => {
    const organization = organizations.find((candidate) => candidate.id === returned.organization);
    if (organization) setCurrentOrganization(organization);
    navigate(SETTINGS);
  };
  const toSettings = <Button onClick={settle}>Go to organization settings</Button>;

  if (!returned.receipt || !returned.organization) {
    return (
      <AuthCard subtitle={TITLE}>
        <AuthStatus
          title="No sign-in to finish"
          description="This page finishes a Claude sign-in claude.com sent back to Zone. Start one from organization settings."
          action={toSettings}
        />
      </AuthCard>
    );
  }

  if (outcome.state === 'failed') {
    return (
      <AuthCard subtitle={TITLE}>
        <AuthStatus
          title="Claude sign-in failed"
          description={outcome.reason}
          action={toSettings}
        />
      </AuthCard>
    );
  }

  if (outcome.state === 'finishing') {
    return (
      <AuthCard subtitle={TITLE}>
        <div className="auth-loading" role="status">
          <span className="spinner" />
          <span>Finishing the sign-in…</span>
        </div>
      </AuthCard>
    );
  }

  return (
    <AuthCard subtitle={TITLE}>
      <div className="auth-success" role="status">
        <div className="success-icon">
          <CheckIcon />
        </div>
        <p className="success-message">Signed in to Claude</p>
        <p className="redirect-message">Claude Code is signed in. You can close this tab.</p>
        {toSettings}
      </div>
    </AuthCard>
  );
}
