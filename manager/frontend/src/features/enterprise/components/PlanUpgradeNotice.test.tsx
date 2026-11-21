import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { Plan } from '../schemas';
import type { Plan as PlanValue } from '../types';
import { LICENSE_PATH, PlanUpgradeNotice } from './PlanUpgradeNotice';

const MESSAGE = 'Spend limits are part of Zone Team. Add a license to set them.';

function renderNotice(plan: PlanValue) {
  return render(
    <MemoryRouter>
      <PlanUpgradeNotice plan={plan} message={MESSAGE} />
    </MemoryRouter>
  );
}

describe('PlanUpgradeNotice', () => {
  it('says a Team feature is available on Team, with the message and a way to the License page', () => {
    renderNotice(Plan.Team);

    const notice = screen.getByRole('note');
    expect(screen.getByRole('heading', { name: 'Available on Team' })).toBeInTheDocument();
    expect(notice).toHaveTextContent(MESSAGE);
    expect(screen.getByRole('link', { name: 'Go to License' })).toHaveAttribute(
      'href',
      LICENSE_PATH
    );
  });

  it('keeps saying Enterprise for an Enterprise feature', () => {
    renderNotice(Plan.Enterprise);

    expect(screen.getByRole('heading', { name: 'Available on Enterprise' })).toBeInTheDocument();
    expect(screen.queryByText(/Available on Team/)).not.toBeInTheDocument();
  });

  it('links to the server License settings', () => {
    expect(LICENSE_PATH).toBe('/settings/server/license');
  });
});
