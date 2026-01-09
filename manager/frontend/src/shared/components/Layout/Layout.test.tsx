import { render, screen } from '@testing-library/react';
import Layout from './Layout';

// Mock Sidebar
jest.mock('../Sidebar/Sidebar', () => {
  return function MockSidebar() {
    return <aside data-testid="sidebar">Sidebar</aside>;
  };
});

// Mock react-router-dom
jest.mock('react-router-dom', () => ({
  Outlet: () => <div data-testid="outlet">Outlet content</div>,
}));

describe('Layout', () => {
  it('renders sidebar', () => {
    render(<Layout />);
    expect(screen.getByTestId('sidebar')).toBeInTheDocument();
  });

  it('renders outlet', () => {
    render(<Layout />);
    expect(screen.getByTestId('outlet')).toBeInTheDocument();
  });

  it('has main content area', () => {
    render(<Layout />);
    expect(document.querySelector('.main-content')).toBeInTheDocument();
  });

  it('has layout class', () => {
    render(<Layout />);
    expect(document.querySelector('.layout')).toBeInTheDocument();
  });
});
