import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import UnauthorizedPage from '../../../pages/UnauthorizedPage';
import EmailVerificationPage from '../pages/EmailVerificationPage';
import ResetPasswordPage from '../pages/ResetPasswordPage';
import AuthCard from './AuthCard';
import AuthStatus from './AuthStatus';

/**
 * Every signed-out error lands on the same anatomy: the auth card's logo row,
 * then the empty-state body (icon, title, description, one button). The pages
 * that used to draw their own are pinned to it here; the invitation page, which
 * needs the auth provider, is pinned in its own test file.
 */

function anatomy(container: HTMLElement) {
  const card = container.querySelector('.auth-container');
  const status = container.querySelector('.auth-error-state');
  return {
    card: Boolean(card),
    logo: Boolean(card?.querySelector('.auth-header .zone-logo')),
    icon: Boolean(status?.querySelector('.error-icon svg')),
    title: status?.querySelector('h2.error-title')?.textContent ?? null,
    description: status?.querySelector('p.error-message')?.textContent ?? null,
    action: Boolean(status?.querySelector('a.btn, button')),
    ownCard: Boolean(container.querySelector('.invitation-card, .unauthorized-card')),
  };
}

describe('AuthCard and AuthStatus', () => {
  it('lay out the logo row over an empty-state body', () => {
    const { container } = render(
      <AuthCard subtitle="What this page is for">
        <AuthStatus
          title="Something is wrong"
          description="And this is why"
          action={<button type="button">Fix it</button>}
        />
      </AuthCard>
    );

    expect(anatomy(container)).toEqual({
      card: true,
      logo: true,
      icon: true,
      title: 'Something is wrong',
      description: 'And this is why',
      action: true,
      ownCard: false,
    });
    expect(screen.getByText('What this page is for').closest('.auth-header')).not.toBeNull();
    expect(screen.getByRole('alert')).toHaveAttribute('aria-live', 'assertive');
  });

  it('are what the unauthorized page renders', () => {
    const { container } = render(
      <BrowserRouter>
        <UnauthorizedPage />
      </BrowserRouter>
    );
    expect(anatomy(container)).toMatchObject({
      card: true,
      logo: true,
      icon: true,
      title: 'Access Denied',
      action: true,
      ownCard: false,
    });
  });

  it('are what an invalid reset link renders', () => {
    window.history.pushState({}, '', '/reset-password?token=not-a-token');
    const { container } = render(
      <BrowserRouter>
        <ResetPasswordPage />
      </BrowserRouter>
    );
    expect(anatomy(container)).toMatchObject({
      card: true,
      logo: true,
      icon: true,
      title: 'Invalid reset link',
      description: 'Invalid token format',
      action: true,
      ownCard: false,
    });
  });

  it('are what a missing verification token renders', () => {
    window.history.pushState({}, '', '/verify-email');
    const { container } = render(
      <BrowserRouter>
        <EmailVerificationPage />
      </BrowserRouter>
    );
    expect(anatomy(container)).toMatchObject({
      card: true,
      logo: true,
      icon: true,
      title: 'Invalid verification link',
      action: true,
      ownCard: false,
    });
  });
});
