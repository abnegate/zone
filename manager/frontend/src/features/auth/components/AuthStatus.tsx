import type { ReactNode } from 'react';

interface AuthStatusProps {
  title: string;
  description?: string;
  icon?: ReactNode;
  action?: ReactNode;
}

const CrossIcon = () => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <circle cx="12" cy="12" r="10" />
    <line x1="15" y1="9" x2="9" y2="15" />
    <line x1="9" y1="9" x2="15" y2="15" />
  </svg>
);

/// The error body of an auth card, laid out like every other empty state: a
/// 32px icon, a 14px title, a 13px line under it and one button.
export default function AuthStatus({ title, description, icon, action }: AuthStatusProps) {
  return (
    <div className="auth-error-state" role="alert" aria-live="assertive">
      <div className="error-icon" data-testid="error-icon">
        {icon ?? <CrossIcon />}
      </div>
      <h2 className="error-title">{title}</h2>
      {description && <p className="error-message">{description}</p>}
      {action}
    </div>
  );
}
