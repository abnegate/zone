import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { StrictMode } from 'react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import fixture from '../../../../../../runner/zone_server/tests/fixtures/agents.json';
import { AgentRequestError } from '../../../api/AgentRequestError';
import type { Organization } from '../organization/types';
import type { AgentStatus } from './schemas';

const agentsApi = { redeem: mock() };
mock.module('../../../api/agents', () => ({ agentsApi }));

const organization: Organization = {
  id: '7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c',
  name: 'Agent sign-ins',
  slug: 'agent-sign-ins',
  description: null,
  is_active: true,
  role: 'owner',
  created_at: '2026-09-23T04:00:00Z',
  updated_at: '2026-09-23T04:00:00Z',
};
const setCurrentOrganization = mock();
mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ organizations: [organization], setCurrentOrganization }),
}));

let AgentSignInPage: typeof import('./AgentSignInPage').default;

beforeAll(async () => {
  AgentSignInPage = (await import('./AgentSignInPage')).default;
});

afterAll(() => {
  mock.restore();
});

const receipt = 'fake-receipt_0123456789';
const [claudeSignedIn] = fixture.agents as AgentStatus[];
const startedElsewhere =
  'Someone else started this Claude sign-in, or it was started in another browser, so Zone did not finish it.';

function Address() {
  const { pathname, search } = useLocation();
  return <output aria-label="Address">{`${pathname}${search}`}</output>;
}

function open(address: string) {
  render(
    <StrictMode>
      <MemoryRouter initialEntries={[address]}>
        <Routes>
          <Route
            path="/agent-sign-in"
            element={
              <>
                <AgentSignInPage />
                <Address />
              </>
            }
          />
          <Route path="/org-settings" element={<h1>Organization settings</h1>} />
        </Routes>
      </MemoryRouter>
    </StrictMode>
  );
}

const returned = `/agent-sign-in?receipt=${receipt}&organization=${organization.id}`;

describe('AgentSignInPage', () => {
  beforeEach(() => {
    agentsApi.redeem.mockReset();
    setCurrentOrganization.mockReset();
  });

  it('hands the receipt in once, drops it from the address, and says the sign-in finished', async () => {
    agentsApi.redeem.mockResolvedValue(claudeSignedIn);

    open(returned);

    expect(await screen.findByText('Signed in to Claude')).toBeInTheDocument();
    expect(agentsApi.redeem).toHaveBeenCalledTimes(1);
    expect(agentsApi.redeem).toHaveBeenCalledWith(organization.id, receipt);
    expect(screen.getByLabelText('Address')).toHaveTextContent(/^\/agent-sign-in$/);
    expect(screen.getByText(/You can close this tab/)).toBeInTheDocument();
  });

  it('says why a sign-in did not finish', async () => {
    agentsApi.redeem.mockRejectedValue(new AgentRequestError(startedElsewhere, 403, 'start_again'));

    open(returned);

    expect(await screen.findByRole('alert')).toHaveTextContent(startedElsewhere);
    expect(screen.getByText('Claude sign-in failed')).toBeInTheDocument();
    expect(screen.queryByText('Signed in to Claude')).toBeNull();
    expect(agentsApi.redeem).toHaveBeenCalledTimes(1);
  });

  it('hands nothing in when the address carries no receipt', async () => {
    open(`/agent-sign-in?organization=${organization.id}`);

    expect(await screen.findByText('No sign-in to finish')).toBeInTheDocument();
    expect(agentsApi.redeem).not.toHaveBeenCalled();
  });

  it('goes on to the settings of the organization it signed in', async () => {
    agentsApi.redeem.mockResolvedValue(claudeSignedIn);
    open(returned);

    fireEvent.click(await screen.findByRole('button', { name: 'Go to organization settings' }));

    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'Organization settings' })).toBeInTheDocument()
    );
    expect(setCurrentOrganization).toHaveBeenCalledWith(organization);
  });
});
