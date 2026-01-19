import { useEffect, useRef, useState } from 'react';
import { useWorkspace } from '../../context/WorkspaceContext';
import './ContextSwitcher.css';

type ContextSwitcherProps = {
  useWorkspaceHook?: typeof useWorkspace;
};

export default function ContextSwitcher({
  useWorkspaceHook = useWorkspace,
}: ContextSwitcherProps) {
  const {
    organizations,
    currentOrganization,
    workspaces,
    currentWorkspace,
    setCurrentOrganization,
    setCurrentWorkspace,
    loading,
  } = useWorkspaceHook();

  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Close dropdown when clicking outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  if (loading) {
    return (
      <div className="context-switcher">
        <div className="context-switcher-loading">Loading...</div>
      </div>
    );
  }

  if (!currentOrganization) {
    return (
      <div className="context-switcher">
        <div className="context-switcher-empty">No organization</div>
      </div>
    );
  }

  return (
    <div className="context-switcher" ref={dropdownRef}>
      <button
        className="context-switcher-button"
        onClick={() => setIsOpen(!isOpen)}
        type="button"
        aria-expanded={isOpen}
        aria-haspopup="listbox"
      >
        <span className="context-label">
          <span className="org-name">{currentOrganization.name}</span>
          {currentWorkspace && (
            <>
              <span className="separator">/</span>
              <span className="ws-name">{currentWorkspace.name}</span>
            </>
          )}
        </span>
        <svg
          className={`chevron ${isOpen ? 'open' : ''}`}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          aria-hidden="true"
        >
          <path d="M6 9l6 6 6-6" />
        </svg>
      </button>

      {isOpen && (
        <div className="context-dropdown" role="listbox">
          <div className="dropdown-section">
            <h4>Organizations</h4>
            {organizations.map((org) => (
              <button
                key={org.id}
                className={`dropdown-item ${org.id === currentOrganization.id ? 'active' : ''}`}
                onClick={() => {
                  setCurrentOrganization(org);
                  setIsOpen(false);
                }}
                type="button"
                role="option"
                aria-selected={org.id === currentOrganization.id}
              >
                <span className="item-name">{org.name}</span>
                {org.id === currentOrganization.id && (
                  <svg
                    className="check-icon"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                    aria-hidden="true"
                  >
                    <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
                  </svg>
                )}
              </button>
            ))}
          </div>

          {workspaces.length > 0 && (
            <div className="dropdown-section">
              <h4>Workspaces</h4>
              {workspaces.map((ws) => (
                <button
                  key={ws.id}
                  className={`dropdown-item ${ws.id === currentWorkspace?.id ? 'active' : ''}`}
                  onClick={() => {
                    setCurrentWorkspace(ws);
                    setIsOpen(false);
                  }}
                  type="button"
                  role="option"
                  aria-selected={ws.id === currentWorkspace?.id}
                >
                  <span className="item-name">{ws.name}</span>
                  {ws.id === currentWorkspace?.id && (
                    <svg
                      className="check-icon"
                      viewBox="0 0 24 24"
                      fill="currentColor"
                      aria-hidden="true"
                    >
                      <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
                    </svg>
                  )}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
