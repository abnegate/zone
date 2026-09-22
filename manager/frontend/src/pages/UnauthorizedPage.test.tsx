import { render, screen } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import UnauthorizedPage from './UnauthorizedPage';

describe('UnauthorizedPage', () => {
  it('renders access denied heading', () => {
    render(
      <BrowserRouter>
        <UnauthorizedPage />
      </BrowserRouter>
    );
    expect(screen.getByRole('heading', { name: 'Access Denied' })).toBeInTheDocument();
  });

  it('renders permission message', () => {
    render(
      <BrowserRouter>
        <UnauthorizedPage />
      </BrowserRouter>
    );
    expect(screen.getByText("You don't have permission to access this page")).toBeInTheDocument();
  });

  it('renders home link', () => {
    render(
      <BrowserRouter>
        <UnauthorizedPage />
      </BrowserRouter>
    );
    expect(screen.getByRole('link', { name: 'Go to Home' })).toHaveAttribute('href', '/');
  });
});

describe('UnauthorizedPage layout', () => {
  it('renders as the auth card with the logo row over an error state', () => {
    const { container } = render(
      <BrowserRouter>
        <UnauthorizedPage />
      </BrowserRouter>
    );
    const card = container.querySelector('.auth-container');
    expect(card).not.toBeNull();
    expect(card?.querySelector('.auth-header .zone-logo')).not.toBeNull();
    const title = screen.getByRole('heading', { name: 'Access Denied' });
    expect(title).toHaveClass('error-title');
    expect(title.closest('.auth-error-state')).not.toBeNull();
    expect(screen.getByTestId('error-icon')).toBeInTheDocument();
    expect(container.querySelector('.unauthorized-card')).toBeNull();
    expect(screen.getByRole('link', { name: 'Go to Home' })).not.toHaveClass('btn-block');
  });
});
