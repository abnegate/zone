import { type ReactNode, useEffect, useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { useAuth } from '../../../features/auth';
import { useNavDrawer } from '../../context/NavDrawerContext';
import { useTheme } from '../../context/ThemeContext';
import ContextSwitcher from '../ContextSwitcher/ContextSwitcher';
import ZoneLogo from '../ZoneLogo';
import './Sidebar.css';

const COLLAPSED_KEY = 'manager_sidebar_collapsed';

const navItems = [
  {
    path: '/chats',
    label: 'Chats',
    icon: 'M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z',
  },
  {
    path: '/projects',
    label: 'Projects',
    icon: 'M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z',
  },
  {
    path: '/tasks',
    label: 'Tasks',
    icon: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4',
  },
  {
    path: '/models',
    label: 'Models',
    icon: 'M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4',
  },
  {
    path: '/wiki',
    label: 'Wiki',
    icon: 'M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253',
  },
  {
    path: '/settings',
    label: 'Settings',
    icon: 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z',
  },
];

export default function Sidebar({ dock }: { dock?: ReactNode }) {
  const { logout } = useAuth();
  const { theme, toggleTheme } = useTheme();
  const { pathname } = useLocation();
  const drawer = useNavDrawer();
  const mobileOpen = drawer?.open ?? false;
  const closeDrawer = drawer?.close ?? (() => {});
  const [narrow, setCollapsed] = useState(() => localStorage.getItem(COLLAPSED_KEY) === 'true');
  // A phone opens the drawer at full width, so the desktop's collapsed choice
  // never leaves it an icon rail with no labels.
  const collapsed = narrow && !mobileOpen;
  const [zoneClient, setZoneClient] = useState(false);

  useEffect(() => {
    let cancelled = false;
    fetch('/__zone/info')
      .then((response) => (response.ok ? response.json() : null))
      .then((data: { client?: boolean } | null) => {
        if (!cancelled) {
          setZoneClient(Boolean(data?.client));
        }
      })
      .catch(() => {
        if (!cancelled) {
          setZoneClient(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const isNavActive = (path: string, isActive: boolean) => {
    if (path === '/chats') {
      return pathname === '/' || pathname === '/chats';
    }
    return isActive;
  };

  const toggleCollapsed = () => {
    setCollapsed((prev) => {
      const next = !prev;
      localStorage.setItem(COLLAPSED_KEY, String(next));
      document.documentElement.setAttribute('data-sidebar-collapsed', String(next));
      return next;
    });
  };

  if (typeof document !== 'undefined') {
    document.documentElement.setAttribute('data-sidebar-collapsed', String(narrow));
  }

  return (
    <>
      <aside className={`sidebar ${mobileOpen ? 'open' : ''} ${collapsed ? 'collapsed' : ''}`}>
        <div className="sidebar-header">
          <button
            className="sidebar-close"
            onClick={closeDrawer}
            type="button"
            aria-label="Close navigation"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              aria-hidden="true"
            >
              <path d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
          <ZoneLogo size="sm" showText={!collapsed} />
          <button
            className="theme-toggle"
            onClick={toggleTheme}
            type="button"
            aria-label={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              {theme === 'dark' ? (
                <path d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
              ) : (
                <path d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
              )}
            </svg>
          </button>
        </div>

        {!collapsed && (
          <div className="sidebar-context">
            <ContextSwitcher />
          </div>
        )}

        <nav className="sidebar-nav">
          {navItems.map((item) => (
            <NavLink
              key={item.path}
              to={item.path}
              className={({ isActive }) =>
                `nav-item ${isNavActive(item.path, isActive) ? 'active' : ''}`
              }
              onClick={closeDrawer}
              title={collapsed ? item.label : undefined}
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d={item.icon} />
              </svg>
              {!collapsed && <span>{item.label}</span>}
            </NavLink>
          ))}
        </nav>

        <div className="sidebar-dock">{dock}</div>

        <div className="sidebar-footer">
          <button
            className="collapse-btn"
            onClick={toggleCollapsed}
            type="button"
            aria-label={narrow ? 'Expand sidebar' : 'Collapse sidebar'}
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              {narrow ? (
                <path d="M13 5l7 7-7 7M5 5l7 7-7 7" />
              ) : (
                <path d="M11 19l-7-7 7-7M19 19l-7-7 7-7" />
              )}
            </svg>
          </button>
          {zoneClient && (
            <a
              className="logout-btn change-server-btn"
              href="/__zone/change-server"
              title={collapsed ? 'Change Server' : undefined}
              onClick={closeDrawer}
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M21 12a9 9 0 11-6.22-8.56M21 3v6h-6" />
              </svg>
              {!collapsed && <span>Change Server</span>}
            </a>
          )}
          <button
            className="logout-btn"
            onClick={logout}
            type="button"
            title={collapsed ? 'Logout' : undefined}
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4M16 17l5-5-5-5M21 12H9" />
            </svg>
            {!collapsed && <span>Logout</span>}
          </button>
        </div>
      </aside>

      {mobileOpen && (
        <div
          className="sidebar-overlay"
          onClick={closeDrawer}
          onKeyDown={(e) => e.key === 'Escape' && closeDrawer()}
          role="button"
          tabIndex={0}
          aria-label="Close menu"
        />
      )}
    </>
  );
}
