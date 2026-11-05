import { describe, expect, it } from 'bun:test';
import { render } from '@testing-library/react';
import { AgentPermissionSummary } from './AgentPermissionSummary';

const chips = (container: HTMLElement) =>
  [...container.querySelectorAll('.agent-permission-chip')].map((chip) => ({
    text: chip.textContent,
    kind: [...chip.classList].find((name) => name.startsWith('agent-permission-chip--')),
  }));

describe('AgentPermissionSummary', () => {
  it('names the control when nothing is loosened, so the trigger is never bare', () => {
    const { container } = render(<AgentPermissionSummary autoApproved={false} hostTools={false} />);
    expect(chips(container)).toEqual([
      { text: 'Permissions', kind: 'agent-permission-chip--neutral' },
    ]);
  });

  it('names auto-approve on its own chip', () => {
    const { container } = render(<AgentPermissionSummary autoApproved hostTools={false} />);
    expect(chips(container)).toEqual([
      { text: 'Auto-approve', kind: 'agent-permission-chip--auto' },
    ]);
  });

  it('flags host tools as a warning chip apart from the approval state', () => {
    const { container } = render(<AgentPermissionSummary autoApproved={false} hostTools />);
    expect(chips(container)).toEqual([
      { text: 'Permissions', kind: 'agent-permission-chip--neutral' },
      { text: 'Host tools', kind: 'agent-permission-chip--host' },
    ]);
  });

  it('keeps both states as separate chips when both apply', () => {
    const { container } = render(<AgentPermissionSummary autoApproved hostTools />);
    expect(chips(container)).toEqual([
      { text: 'Auto-approve', kind: 'agent-permission-chip--auto' },
      { text: 'Host tools', kind: 'agent-permission-chip--host' },
    ]);
  });

  it('hides its icons from assistive tech, which reads the trigger label instead', () => {
    const { container } = render(<AgentPermissionSummary autoApproved hostTools />);
    const icons = [...container.querySelectorAll('svg')];
    expect(icons).toHaveLength(2);
    for (const icon of icons) expect(icon).toHaveAttribute('aria-hidden', 'true');
  });
});
