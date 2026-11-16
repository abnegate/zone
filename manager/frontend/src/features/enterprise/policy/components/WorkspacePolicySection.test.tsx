import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { Edition, LicenseStatus } from '../../schemas';
import { OPEN_RULES } from '../draft';
import {
  ACTIVE_WORKSPACE_INHERITED,
  ACTIVE_WORKSPACE_OVERRIDDEN,
  ORGANIZATION_ID,
  ORGANIZATION_RULES,
  WORKSPACE_ID,
} from '../fixtures';
import { NOT_ALLOWED } from '../messages';
import { PolicyRefusal } from '../PolicyRefusal';
import { PolicyCode, PolicyTier } from '../schemas';
import type { WorkspacePolicyResponse } from '../types';
import { SAVE_LABEL } from './PolicyEditor';
import { EFFECTIVE_SUMMARY, ORGANIZATION_SUMMARY } from './PolicyFieldFrame';
import { LAPSED_NOTICE } from './PolicyLapsedNotice';
import { INHERIT_LABEL } from './PolicyListField';
import { SOURCE_LABELS } from './PolicySourceBadge';
import { UPGRADE_NOTICE } from './PolicyUpgradeNotice';
import { WORKSPACE_READ_ONLY } from './WorkspacePolicy';
import { ORGANIZATION_POLICY_LINK } from './WorkspacePolicySection';

const workspace = mock();
const updateWorkspace = mock();

mock.module('../api', () => ({
  policyApi: { organization: mock(), updateOrganization: mock(), workspace, updateWorkspace },
}));

let WorkspacePolicySection: typeof import('./WorkspacePolicySection').WorkspacePolicySection;

beforeAll(async () => {
  ({ WorkspacePolicySection } = await import('./WorkspacePolicySection'));
});

afterAll(() => {
  mock.restore();
});

const ORGANIZATION_POLICY_PATH = '/settings/organization/agent-policy';

function renderSection(organizationPolicyPath?: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <WorkspacePolicySection
          organizationId={ORGANIZATION_ID}
          workspaceId={WORKSPACE_ID}
          organizationPolicyPath={organizationPolicyPath}
        />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

async function loaded(policy: WorkspacePolicyResponse, organizationPolicyPath?: string) {
  workspace.mockResolvedValue(policy);
  renderSection(organizationPolicyPath);
  await screen.findByRole('heading', { name: 'Agent policy' });
}

function field(name: string): HTMLElement {
  return screen.getByRole('group', { name });
}

function badge(name: string): string | null {
  return field(name).querySelector('.policy-badge')?.textContent ?? null;
}

function summary(name: string, label: string): string | null {
  const row = [...field(name).querySelectorAll('.policy-field-summary')].find(
    (element) => element.querySelector('.policy-field-summary-label')?.textContent === label
  );
  return row?.querySelector('.policy-field-summary-value')?.textContent ?? null;
}

describe('WorkspacePolicySection', () => {
  beforeEach(() => {
    workspace.mockReset();
    updateWorkspace.mockReset();
  });

  it('shows Community the upgrade notice and no editor', async () => {
    await loaded({
      ...ACTIVE_WORKSPACE_INHERITED,
      entitled: false,
      enforced: false,
      edition: Edition.Community,
      status: LicenseStatus.Community,
      editable: false,
      organization: OPEN_RULES,
      effective: OPEN_RULES,
    });

    expect(screen.getByText(UPGRADE_NOTICE)).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: 'Providers' })).not.toBeInTheDocument();
  });

  it('marks inherited and unrestricted fields and shows the organization and effective values', async () => {
    await loaded(ACTIVE_WORKSPACE_INHERITED);

    expect(badge('Providers')).toBe(SOURCE_LABELS.organization);
    expect(badge('MCP servers')).toBe(SOURCE_LABELS.none);
    expect(badge('Tool approval')).toBe(SOURCE_LABELS.organization);
    expect(summary('Providers', ORGANIZATION_SUMMARY)).toBe(
      'Local models, Anthropic API, AWS Bedrock'
    );
    expect(summary('Providers', EFFECTIVE_SUMMARY)).toBe(
      'Local models, Anthropic API, AWS Bedrock'
    );
    expect(summary('Models', ORGANIZATION_SUMMARY)).toBe('claude-*, qwen3');
    expect(within(field('Providers')).queryByRole('checkbox')).not.toBeInTheDocument();
    expect(
      within(field('Providers')).getByRole('button', { name: 'Override Providers' })
    ).toBeEnabled();
    expect(
      within(field('Tool approval')).getByRole('radio', { name: INHERIT_LABEL })
    ).toBeChecked();
  });

  it('marks overridden fields as set for this workspace and shows the narrowed effective value', async () => {
    await loaded(ACTIVE_WORKSPACE_OVERRIDDEN);

    expect(badge('Providers')).toBe(SOURCE_LABELS.workspace);
    expect(badge('Models')).toBe(SOURCE_LABELS.organization);
    expect(badge('Outbound domains')).toBe(SOURCE_LABELS.workspace);
    expect(summary('Providers', EFFECTIVE_SUMMARY)).toBe('Local models');
    expect(summary('Outbound domains', EFFECTIVE_SUMMARY)).toBe('github.com');
    expect(
      within(field('Tool approval')).getByRole('radio', {
        name: 'Ask before host tools such as shell and file writes',
      })
    ).toBeChecked();
  });

  it('disables providers and agents the organization does not allow, with the reason', async () => {
    await loaded(ACTIVE_WORKSPACE_OVERRIDDEN);

    const providers = field('Providers');
    const openai = within(providers).getByRole('checkbox', { name: 'OpenAI API' });
    expect(openai).toBeDisabled();
    const description = openai.getAttribute('aria-describedby');
    expect(description).not.toBeNull();
    expect(document.getElementById(description ?? '')).toHaveTextContent(NOT_ALLOWED);
    expect(within(providers).getByRole('checkbox', { name: 'Anthropic API' })).toBeEnabled();
    expect(within(providers).getByRole('checkbox', { name: 'Local models' })).toBeChecked();
    expect(within(providers).getAllByText(NOT_ALLOWED)).toHaveLength(4);
  });

  it('disables a ceiling above the organization ceiling, with the reason', async () => {
    await loaded({
      ...ACTIVE_WORKSPACE_INHERITED,
      organization: { ...ORGANIZATION_RULES, ceiling: PolicyTier.Write },
      effective: { ...ORGANIZATION_RULES, ceiling: PolicyTier.Write },
    });

    const ceiling = field('Tool approval');
    const outward = within(ceiling).getByRole('radio', {
      name: `Ask before outward tools such as messages, pull requests, web and MCP ${NOT_ALLOWED}`,
    });
    expect(outward).toBeDisabled();
    expect(
      within(ceiling).getByRole('radio', {
        name: 'Ask before host tools such as shell and file writes',
      })
    ).toBeEnabled();
    expect(within(ceiling).getByRole('radio', { name: INHERIT_LABEL })).toBeEnabled();
  });

  it('overrides an inherited field starting from the organization value and saves only it', async () => {
    updateWorkspace.mockResolvedValue(ACTIVE_WORKSPACE_OVERRIDDEN);
    await loaded(ACTIVE_WORKSPACE_INHERITED);

    fireEvent.click(within(field('Providers')).getByRole('button', { name: 'Override Providers' }));
    expect(badge('Providers')).toBe(SOURCE_LABELS.workspace);
    fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'Anthropic API' }));
    fireEvent.click(screen.getByRole('button', { name: SAVE_LABEL }));

    await waitFor(() =>
      expect(updateWorkspace).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID, {
        providers: ['local', 'bedrock'],
      })
    );
  });

  it('sends null to go back to the organization policy', async () => {
    updateWorkspace.mockResolvedValue(ACTIVE_WORKSPACE_INHERITED);
    await loaded(ACTIVE_WORKSPACE_OVERRIDDEN);

    fireEvent.click(
      within(field('Providers')).getByRole('button', { name: `${INHERIT_LABEL} for Providers` })
    );
    fireEvent.click(within(field('Tool approval')).getByRole('radio', { name: INHERIT_LABEL }));
    expect(badge('Providers')).toBe(SOURCE_LABELS.organization);
    fireEvent.click(screen.getByRole('button', { name: SAVE_LABEL }));

    await waitFor(() =>
      expect(updateWorkspace).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_ID, {
        providers: null,
        ceiling: null,
      })
    );
  });

  it("shows a widening refusal as a form alert in the server's words", async () => {
    const worded =
      "This workspace's providers can't allow more than your organization's policy does.";
    updateWorkspace.mockRejectedValue(
      new PolicyRefusal({ error: worded, code: PolicyCode.Wider }, 400)
    );
    await loaded(ACTIVE_WORKSPACE_OVERRIDDEN);

    fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'Anthropic API' }));
    fireEvent.click(screen.getByRole('button', { name: SAVE_LABEL }));

    expect(await screen.findByRole('alert')).toHaveTextContent(worded);
  });

  it('links organization admins to the organization policy', async () => {
    await loaded(ACTIVE_WORKSPACE_INHERITED, ORGANIZATION_POLICY_PATH);

    expect(screen.getByRole('link', { name: ORGANIZATION_POLICY_LINK })).toHaveAttribute(
      'href',
      ORGANIZATION_POLICY_PATH
    );
  });

  it('offers no organization link to anyone else', async () => {
    await loaded(ACTIVE_WORKSPACE_INHERITED);

    expect(screen.queryByRole('link', { name: ORGANIZATION_POLICY_LINK })).not.toBeInTheDocument();
  });

  it('shows the workspace policy read-only to someone who cannot edit it', async () => {
    await loaded({ ...ACTIVE_WORKSPACE_OVERRIDDEN, editable: false });

    expect(screen.getByText(WORKSPACE_READ_ONLY)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Override|Use organization policy/ })).toBeNull();
    expect(
      within(field('Providers')).getByRole('checkbox', { name: 'Local models' })
    ).toBeDisabled();
    expect(screen.queryByRole('button', { name: SAVE_LABEL })).not.toBeInTheDocument();
  });

  it('shows a lapsed workspace policy read-only under the lapsed notice', async () => {
    await loaded({
      ...ACTIVE_WORKSPACE_OVERRIDDEN,
      entitled: false,
      editable: false,
      status: LicenseStatus.Lapsed,
    });

    expect(screen.getByRole('status')).toHaveTextContent(LAPSED_NOTICE);
    expect(screen.queryByText(WORKSPACE_READ_ONLY)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: SAVE_LABEL })).not.toBeInTheDocument();
  });
});
