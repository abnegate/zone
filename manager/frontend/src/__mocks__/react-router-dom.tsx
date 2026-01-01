import type { ReactNode } from 'react';

// Mock implementations
export const useNavigate = jest.fn(() => jest.fn());
export const useLocation = jest.fn(() => ({
  pathname: '/',
  state: null,
  search: '',
  hash: '',
  key: '',
}));
export const useParams = jest.fn(() => ({}));
export const useSearchParams = jest.fn(() => [new URLSearchParams(), jest.fn()]);

// Mock components
export const BrowserRouter = ({ children }: { children: ReactNode }) => <>{children}</>;
export const MemoryRouter = ({
  children,
  initialEntries = ['/'],
}: { children: ReactNode; initialEntries?: string[] }) => <>{children}</>;
export const Routes = ({ children }: { children: ReactNode }) => <>{children}</>;
export const Route = ({ element }: { path?: string; element?: ReactNode; index?: boolean }) => (
  <>{element}</>
);
export const Navigate = ({ to, replace }: { to: string; replace?: boolean }) => null;
export const Outlet = () => null;

export const Link = ({
  to,
  children,
  ...props
}: { to: string; children: ReactNode; [key: string]: unknown }) => (
  <a href={typeof to === 'string' ? to : '/'} {...props}>
    {children}
  </a>
);

export const NavLink = Link;

export default {
  useNavigate,
  useLocation,
  useParams,
  useSearchParams,
  BrowserRouter,
  MemoryRouter,
  Routes,
  Route,
  Navigate,
  Outlet,
  Link,
  NavLink,
};
