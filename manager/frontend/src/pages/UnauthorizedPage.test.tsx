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
  it('sets the title in plain ink on one card', () => {
    const { container } = render(
      <BrowserRouter>
        <UnauthorizedPage />
      </BrowserRouter>
    );
    const title = screen.getByRole('heading', { name: 'Access Denied' });
    expect(title).toHaveClass('unauthorized-title');
    expect(title.getAttribute('style')).toBeNull();
    expect(container.querySelector('.unauthorized-card')).not.toBeNull();
    expect(container.querySelector('.auth-container')).toBeNull();
    expect(screen.getByRole('link', { name: 'Go to Home' })).not.toHaveClass('btn-block');
  });
});
