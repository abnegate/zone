import type { ReactNode } from 'react';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import '../pages/AuthPage.css';

interface AuthCardProps {
  subtitle?: string;
  children: ReactNode;
}

/// The one card every signed-out page sits on: the logo row, an optional
/// line under it saying what the page is for, then whatever the page shows.
export default function AuthCard({ subtitle, children }: AuthCardProps) {
  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <ZoneLogo size="md" />
          {subtitle && <p>{subtitle}</p>}
        </div>
        {children}
      </div>
    </div>
  );
}
