import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describedText } from '../../../../test/select';
import { ORGANIZATION_RULES, WORKSPACE_RULES } from '../fixtures';
import { NOTHING_ALLOWED_WARNING, notAllowed, pendingText, refusalMessage } from '../messages';
import { PolicyRefusal } from '../PolicyRefusal';
import { PolicyCode, PolicyProvider } from '../schemas';
import { PolicyScope } from '../source';
import type { PolicyRules } from '../types';
import { LEGENDS, PolicyEditor, SAVE_LABEL } from './PolicyEditor';

interface EditorOptions {
  scope?: PolicyScope;
  saved?: PolicyRules;
  organization?: PolicyRules;
  refusal?: PolicyRefusal | null;
}

function editor(
  {
    scope = PolicyScope.Organization,
    saved = ORGANIZATION_RULES,
    organization,
    refusal = null,
  }: EditorOptions,
  onSave = mock()
) {
  return (
    <PolicyEditor
      scope={scope}
      saved={saved}
      organization={organization}
      effective={organization}
      editable
      pending={false}
      refusal={refusal}
      error={refusal?.message ?? null}
      onSave={onSave}
      onReset={() => {}}
    />
  );
}

function field(name: string): HTMLElement {
  return screen.getByRole('group', { name });
}

function save() {
  fireEvent.click(screen.getByRole('button', { name: SAVE_LABEL }));
}

describe('PolicyEditor', () => {
  it('names its fields for assistive technology', () => {
    render(editor({}));

    expect(
      screen.getByRole('group', { name: LEGENDS[PolicyScope.Organization] })
    ).toBeInTheDocument();
  });

  it("keeps another admin's change to a field this draft never touched", () => {
    const onSave = mock();
    const view = render(editor({}, onSave));

    fireEvent.click(within(field('Providers')).getByRole('checkbox', { name: 'OpenAI API' }));
    const narrowed: PolicyRules = { ...ORGANIZATION_RULES, domains: ['github.com'] };
    view.rerender(editor({ saved: narrowed }, onSave));

    expect(within(field('Outbound domains')).queryByText('example.com')).toBeNull();
    save();
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0]).toEqual({
      providers: [
        PolicyProvider.Local,
        PolicyProvider.Anthropic,
        PolicyProvider.Bedrock,
        PolicyProvider.OpenAI,
      ],
    });
  });

  it('adds text typed into a list but never added when saving, and says it will', () => {
    const onSave = mock();
    render(editor({}, onSave));
    const servers = field('MCP servers');

    fireEvent.click(within(servers).getByRole('checkbox', { name: 'Restrict MCP servers' }));
    const entry = within(servers).getByRole('textbox', { name: 'Add to MCP servers' });
    fireEvent.change(entry, { target: { value: 'github' } });

    expect(within(servers).getByText(pendingText(['github']))).toBeInTheDocument();
    expect(describedText(entry)).toBe(pendingText(['github']));
    save();
    expect(onSave.mock.calls[0][0]).toEqual({ mcp_servers: ['github'] });
  });

  it('enables saving for typed text alone', () => {
    const onSave = mock();
    render(editor({}, onSave));

    fireEvent.change(within(field('Models')).getByRole('textbox', { name: 'Add to Models' }), {
      target: { value: 'gpt-5*' },
    });
    save();

    expect(onSave.mock.calls[0][0]).toEqual({ models: ['claude-*', 'qwen3', 'gpt-5*'] });
  });

  it('warns that an empty restricted list allows nothing', () => {
    render(editor({}));
    const servers = field('MCP servers');

    fireEvent.click(within(servers).getByRole('checkbox', { name: 'Restrict MCP servers' }));

    expect(within(servers).getByText(NOTHING_ALLOWED_WARNING)).toHaveClass('policy-field-warning');
    fireEvent.change(within(servers).getByRole('textbox', { name: 'Add to MCP servers' }), {
      target: { value: 'github' },
    });
    expect(within(servers).queryByText(NOTHING_ALLOWED_WARNING)).toBeNull();
  });

  describe('in a workspace', () => {
    const workspace = {
      scope: PolicyScope.Workspace,
      saved: WORKSPACE_RULES,
      organization: ORGANIZATION_RULES,
    };

    it('refuses an entry the organization does not cover when it is added', () => {
      render(editor(workspace));
      const domains = field('Outbound domains');
      const entry = within(domains).getByRole('textbox', { name: 'Add to Outbound domains' });

      fireEvent.change(entry, { target: { value: 'evil.com' } });
      fireEvent.click(within(domains).getByRole('button', { name: 'Add to Outbound domains' }));

      const alert = within(domains).getByRole('alert');
      expect(alert).toHaveTextContent(notAllowed(['evil.com']));
      expect(entry).toHaveAttribute('aria-invalid', 'true');
      expect(entry.getAttribute('aria-describedby')).toContain(alert.id);
      expect(within(domains).queryByText('evil.com', { selector: '.policy-chip-text' })).toBeNull();
      expect(within(domains).queryByText(pendingText(['evil.com']))).toBeNull();
    });

    it('accepts an entry the organization covers', () => {
      render(editor(workspace));
      const domains = field('Outbound domains');

      fireEvent.change(within(domains).getByRole('textbox', { name: 'Add to Outbound domains' }), {
        target: { value: 'API.example.com' },
      });
      fireEvent.click(within(domains).getByRole('button', { name: 'Add to Outbound domains' }));

      expect(within(domains).getByText('api.example.com')).toBeInTheDocument();
      expect(within(domains).queryByRole('alert')).toBeNull();
    });

    it('refuses to save typed text the organization does not cover', () => {
      const onSave = mock();
      render(editor({ ...workspace, saved: { ...WORKSPACE_RULES, models: [] } }, onSave));
      const models = field('Models');

      fireEvent.change(within(models).getByRole('textbox', { name: 'Add to Models' }), {
        target: { value: 'gpt-5' },
      });
      save();

      expect(onSave).not.toHaveBeenCalled();
      expect(within(models).getByRole('alert')).toHaveTextContent(notAllowed(['gpt-5']));
    });

    it("shows the server's own words when it finds the workspace wider", () => {
      const worded =
        "This workspace's MCP servers can't allow more than your organization's policy does.";
      render(
        editor({
          ...workspace,
          refusal: new PolicyRefusal({ error: worded, code: PolicyCode.Wider }, 400),
        })
      );

      expect(screen.getByRole('alert')).toHaveTextContent(worded);
      expect(screen.queryByText(refusalMessage(PolicyCode.Wider))).toBeNull();
    });
  });
});
