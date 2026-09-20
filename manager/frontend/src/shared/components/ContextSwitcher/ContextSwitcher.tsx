import { Button } from '@zone/ui';
import { useEffect, useRef, useState } from 'react';
import { useWorkspace } from '../../context/WorkspaceContext';
import './ContextSwitcher.css';

type ContextSwitcherProps = {
  useWorkspaceHook?: typeof useWorkspace;
};

function CheckIcon() {
  return (
    <svg className="check-icon" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
    </svg>
  );
}

export default function ContextSwitcher({ useWorkspaceHook = useWorkspace }: ContextSwitcherProps) {
  const {
    organizations,
    currentOrganization,
    workspaces,
    currentWorkspace,
    setCurrentOrganization,
    setCurrentWorkspace,
    refreshOrganizations,
    loading,
    error,
  } = useWorkspaceHook();

  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  if (loading && !currentOrganization) {
    return (
      <div className="context-switcher">
        <div
          className="context-switcher-placeholder"
          role="status"
          aria-busy="true"
          aria-label="Loading organizations"
        />
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
          {currentOrganization ? (
            <>
              <span className="org-name">{currentOrganization.name}</span>
              {currentWorkspace && <span className="ws-name">{currentWorkspace.name}</span>}
            </>
          ) : (
            <span className="ws-name context-prompt">Select organization</span>
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
            {organizations.length === 0 && (
              <div className="dropdown-empty">
                <span>{error ? 'Could not load' : 'No organizations yet'}</span>
                {error && (
                  <Button variant="ghost" size="sm" onClick={() => refreshOrganizations()}>
                    Retry
                  </Button>
                )}
              </div>
            )}
            {organizations.map((org) => {
              const active = org.id === currentOrganization?.id;
              return (
                <button
                  key={org.id}
                  className={`dropdown-item ${active ? 'active' : ''}`}
                  onClick={() => {
                    setCurrentOrganization(org);
                    setIsOpen(false);
                  }}
                  type="button"
                  role="option"
                  aria-selected={active}
                >
                  <span className="item-name">{org.name}</span>
                  {active && <CheckIcon />}
                </button>
              );
            })}
          </div>

          {workspaces.length > 0 && (
            <div className="dropdown-section">
              <h4>Workspaces</h4>
              {workspaces.map((ws) => {
                const active = ws.id === currentWorkspace?.id;
                return (
                  <button
                    key={ws.id}
                    className={`dropdown-item ${active ? 'active' : ''}`}
                    onClick={() => {
                      setCurrentWorkspace(ws);
                      setIsOpen(false);
                    }}
                    type="button"
                    role="option"
                    aria-selected={active}
                  >
                    <span className="item-name">{ws.name}</span>
                    {active && <CheckIcon />}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
