import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import {
  ACTIVE_ORGANIZATION,
  COMMUNITY_ORGANIZATION,
  LAPSED_ORGANIZATION,
  ORGANIZATION_ID,
} from '../fixtures';
import { NOTHING_ALLOWED, refusalMessage, unrestrictedText } from '../messages';
import { PolicyRefusal } from '../PolicyRefusal';
import { PolicyCode, PolicyField } from '../schemas';
import type { OrganizationPolicyResponse } from '../types';
import { ORGANIZATION_READ_ONLY } from './OrganizationPolicySection';
import { DISCARD_LABEL, SAVE_LABEL } from './PolicyEditor';
import { LAPSED_NOTICE } from './PolicyLapsedNotice';
import { UPGRADE_NOTICE } from './PolicyUpgradeNotice';

const organization = mock();
const updateOrganization = mock();

mock.module('../api', () => ({
  policyApi: { organization, updateOrganization, workspace: mock(), updateWorkspace: mock() },
}));

let OrganizationPolicySection: typeof import('./OrganizationPolicySection').OrganizationPolicySection;

beforeAll(async () => {
  ({ OrganizationPolicySection } = await import('./OrganizationPolicySection'));
});

afterAll(() => {
  mock.restore();
});

function renderSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <OrganizationPolicySection organizationId={ORGANIZATION_ID} />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

async function loaded(policy: OrganizationPolicyResponse) {
  organization.mockResolvedValue(policy);
  renderSection();
  await screen.findByRole('heading', { name: 'Agent policy' });
}

function field(name: string): HTMLElement {
  return screen.getByRole('group', { name });
}

function save(): HTMLElement {
  return screen.getByRole('button', { name: SAVE_LABEL });
}

describe('OrganizationPolicySection', () => {
  beforeEach(() => {
    organization.mockReset();
    updateOrganization.mockReset();
  });

  describe('states', () => {
    it('shows a loading state until the policy arrives', () => {
      organization.mockImplementation(() => new Promise(() => {}));
      renderSection();

      expect(screen.getByText('Loading the agent policy...')).toBeInTheDocument();
    });

    it('offers a retry when the policy fails to load', async () => {
      organization
        .mockRejectedValueOnce(new ApiError('Failed to load the agent policy: Not found', 404))
        .mockResolvedValueOnce(ACTIVE_ORGANIZATION);
      renderSection();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load the agent policy: Not found');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByRole('group', { name: 'Providers' });
      expect(organization).toHaveBeenCalledTimes(2);
    });

    it('shows Community the upgrade notice with a way to the License page and no editor', async () => {
      await loaded(COMMUNITY_ORGANIZATION);

      expect(screen.getByText(UPGRADE_NOTICE)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Go to License' })).toHaveAttribute(
        'href',
        '/settings/server/license'
      );
      expect(screen.queryByRole('group', { name: 'Providers' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_LABEL })).not.toBeInTheDocument();
    });

    it('warns that a lapsed license keeps enforcing and shows the policy read-only', async () => {
      await loaded(LAPSED_ORGANIZATION);

      const warning = screen.getByRole('status');
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent(LAPSED_NOTICE);
      expect(warning).toHaveTextContent(
        'Policies are still enforced; editing needs an active or grace license'
      );
      expect(within(warning).getByRole('link')).toHaveAttribute('href', '/settings/server/license');
      expect(screen.queryByText(UPGRADE_NOTICE)).not.toBeInTheDocument();
      expect(
        within(field('Providers')).getByRole('checkbox', { name: 'Local models' })
      ).toBeDisabled();
      expect(within(field('Models')).queryByRole('button')).not.toBeInTheDocument();
      expect(within(field('Models')).getByText('claude-*')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_LABEL })).not.toBeInTheDocument();
      expect(screen.queryByText(ORGANIZATION_READ_ONLY)).not.toBeInTheDocument();
    });

    it('shows a member who cannot edit the policy read-only', async () => {
      await loaded({ ...ACTIVE_ORGANIZATION, editable: false });

      expect(screen.getByText(ORGANIZATION_READ_ONLY)).toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
      for (const checkbox of within(field('Providers')).getAllByRole('checkbox')) {
        expect(checkbox).toBeDisabled();
      }
      for (const radio of within(field('Tool approval')).getAllByRole('radio')) {
        expect(radio).toBeDisabled();
      }
      expect(within(field('Outbound domains')).queryByRole('textbox')).not.toBeInTheDocument();
      expect(
        screen.queryByRole('checkbox', { name: 'Restrict Providers' })
      ).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_LABEL })).not.toBeInTheDocument();
    });
  });

  describe('editing', () => {
    it('shows the saved policy with what each unrestricted field allows', async () => {
      await loaded(ACTIVE_ORGANIZATION);

      const providers = field('Providers');
      expect(within(providers).getByRole('checkbox', { name: 'Restrict Providers' })).toBeChecked();
      expect(within(providers).getByRole('checkbox', { name: 'AWS Bedrock' })).toBeChecked();
      expect(within(providers).getByRole('checkbox', { name: 'OpenAI API' })).not.toBeChecked();
      expect(
        within(field('MCP servers')).getByText(unrestrictedText(PolicyField.McpServers))
      ).toBeInTheDocument();
      expect(
        within(field('Tool approval')).getByRole('radio', {
          name: 'Ask before outward tools such as messages, pull requests, web and MCP',
        })
      ).toBeChecked();
      expect(save()).toBeDisabled();
    });

    it('saves only the fields that changed', async () => {
      updateOrganization.mockResolvedValue(ACTIVE_ORGANIZATION);
      await loaded(ACTIVE_ORGANIZATION);

      fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'AWS Bedrock' }));
      const models = field('Models');
      fireEvent.change(within(models).getByRole('textbox', { name: 'Add to Models' }), {
        target: { value: 'gpt-5*' },
      });
      fireEvent.keyDown(within(models).getByRole('textbox', { name: 'Add to Models' }), {
        key: 'Enter',
      });
      expect(within(models).getByText('gpt-5*')).toBeInTheDocument();
      fireEvent.click(save());

      await waitFor(() => expect(updateOrganization).toHaveBeenCalledTimes(1));
      expect(updateOrganization).toHaveBeenCalledWith(ORGANIZATION_ID, {
        providers: ['local', 'anthropic'],
        models: ['claude-*', 'qwen3', 'gpt-5*'],
      });
    });

    it('sends null when a field stops being restricted or the ceiling is lifted', async () => {
      updateOrganization.mockResolvedValue(ACTIVE_ORGANIZATION);
      await loaded(ACTIVE_ORGANIZATION);

      fireEvent.click(
        within(field('Outbound domains')).getByRole('checkbox', {
          name: 'Restrict Outbound domains',
        })
      );
      expect(
        within(field('Outbound domains')).getByText(unrestrictedText(PolicyField.Domains))
      ).toBeInTheDocument();
      fireEvent.click(within(field('Tool approval')).getByRole('radio', { name: 'No limit' }));
      fireEvent.click(save());

      await waitFor(() =>
        expect(updateOrganization).toHaveBeenCalledWith(ORGANIZATION_ID, {
          ceiling: null,
          domains: null,
        })
      );
    });

    it('starts a newly restricted list empty, which allows nothing until an entry is added', async () => {
      updateOrganization.mockResolvedValue(ACTIVE_ORGANIZATION);
      await loaded(ACTIVE_ORGANIZATION);

      const servers = field('MCP servers');
      fireEvent.click(within(servers).getByRole('checkbox', { name: 'Restrict MCP servers' }));
      expect(within(servers).getByText(NOTHING_ALLOWED)).toBeInTheDocument();
      fireEvent.change(within(servers).getByRole('textbox'), { target: { value: 'github' } });
      fireEvent.click(within(servers).getByRole('button', { name: 'Add to MCP servers' }));
      fireEvent.click(save());

      await waitFor(() =>
        expect(updateOrganization).toHaveBeenCalledWith(ORGANIZATION_ID, {
          mcp_servers: ['github'],
        })
      );
    });

    it('removes an entry and discards changes back to the saved policy', async () => {
      await loaded(ACTIVE_ORGANIZATION);

      fireEvent.click(
        within(field('Outbound domains')).getByRole('button', { name: 'Remove github.com' })
      );
      expect(within(field('Outbound domains')).queryByText('github.com')).not.toBeInTheDocument();
      expect(save()).toBeEnabled();

      fireEvent.click(screen.getByRole('button', { name: DISCARD_LABEL }));

      expect(within(field('Outbound domains')).getByText('github.com')).toBeInTheDocument();
      expect(save()).toBeDisabled();
      expect(updateOrganization).not.toHaveBeenCalled();
    });
  });

  describe('refusals', () => {
    const inline = [
      { code: PolicyCode.ModelInvalid, name: 'Models' },
      { code: PolicyCode.DomainInvalid, name: 'Outbound domains' },
      { code: PolicyCode.ServerInvalid, name: 'MCP servers' },
      { code: PolicyCode.CeilingInvalid, name: 'Tool approval' },
    ] as const;

    for (const { code, name } of inline) {
      it(`shows ${code} on the ${name} field`, async () => {
        updateOrganization.mockRejectedValue(new PolicyRefusal({ error: 'raw', code }, 400));
        await loaded(ACTIVE_ORGANIZATION);

        fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'OpenAI API' }));
        fireEvent.click(save());

        const error = await within(field(name)).findByRole('alert');
        expect(error).toHaveTextContent(refusalMessage(code));
        expect(screen.getAllByRole('alert')).toHaveLength(1);
      });
    }

    for (const code of [PolicyCode.LicenseRequired, PolicyCode.BodyInvalid, PolicyCode.Wider]) {
      it(`shows ${code} as a form alert`, async () => {
        updateOrganization.mockRejectedValue(new PolicyRefusal({ error: 'raw', code }, 403));
        await loaded(ACTIVE_ORGANIZATION);

        fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'OpenAI API' }));
        fireEvent.click(save());

        const alert = await screen.findByRole('alert');
        expect(alert).toHaveClass('alert-error');
        expect(alert).toHaveTextContent(refusalMessage(code));
        expect(alert).not.toHaveTextContent('raw');
      });
    }

    it('clears a refusal once the policy is edited again', async () => {
      updateOrganization.mockRejectedValue(
        new PolicyRefusal({ error: 'raw', code: PolicyCode.LicenseRequired }, 403)
      );
      await loaded(ACTIVE_ORGANIZATION);

      fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'OpenAI API' }));
      fireEvent.click(save());
      await screen.findByRole('alert');

      fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'Azure OpenAI' }));

      await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
    });
  });
});
