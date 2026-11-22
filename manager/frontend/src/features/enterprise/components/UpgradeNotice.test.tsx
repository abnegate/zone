import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { LICENSE_PATH } from '../licensePath';
import { Plan } from '../schemas';
import type { Plan as PlanValue } from '../types';
import { AdministersServerProvider } from './AdministersServer';
import { ASK_FOR_LICENSE, LICENSE_LINK, UpgradeNotice } from './UpgradeNotice';

const MESSAGE = 'Spend limits are part of Zone Team. Add a license to set them.';

function renderNotice(plan: PlanValue | undefined, administersServer = true) {
  return render(
    <MemoryRouter>
      <AdministersServerProvider value={administersServer}>
        <UpgradeNotice plan={plan} message={MESSAGE} className="plan-notice" />
      </AdministersServerProvider>
    </MemoryRouter>
  );
}

describe('UpgradeNotice', () => {
  it('says a Team feature is available on Team, with the message and a way to the License page', () => {
    renderNotice(Plan.Team);

    const notice = screen.getByRole('note');
    expect(notice).toHaveClass('plan-notice');
    expect(screen.getByRole('heading', { name: 'Available on Team' })).toBeInTheDocument();
    expect(notice).toHaveTextContent(MESSAGE);
    expect(screen.getByRole('link', { name: LICENSE_LINK })).toHaveAttribute('href', LICENSE_PATH);
  });

  it('keeps saying Enterprise for an Enterprise feature', () => {
    renderNotice(Plan.Enterprise);

    expect(screen.getByRole('heading', { name: 'Available on Enterprise' })).toBeInTheDocument();
    expect(screen.queryByText(/Available on Team/)).not.toBeInTheDocument();
  });

  it('shows no plan heading when no plan is named', () => {
    renderNotice(undefined);

    expect(screen.getByRole('note')).toHaveTextContent(MESSAGE);
    expect(screen.queryByRole('heading')).not.toBeInTheDocument();
  });

  it('asks someone who does not run the server to request a license instead of linking', () => {
    renderNotice(Plan.Team, false);

    const notice = screen.getByRole('note');
    expect(screen.getByRole('heading', { name: 'Available on Team' })).toBeInTheDocument();
    expect(notice).toHaveTextContent(MESSAGE);
    expect(notice).toHaveTextContent(ASK_FOR_LICENSE);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });
});
