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

describe('UnauthorizedPage title', () => {
  it('sets the title in plain ink instead of a gradient', () => {
    render(
      <BrowserRouter>
        <UnauthorizedPage />
      </BrowserRouter>
    );
    const heading = screen.getByRole('heading', { name: 'Access Denied' });
    expect(heading.getAttribute('style')).toBeNull();
    expect(heading.closest('.auth-container')).not.toBeNull();
  });
});
