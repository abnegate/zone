import type { ReactNode } from 'react';
import BarLead, { type BarBack } from './BarLead';

type PageBarProps = {
  title: string;
  subtitle?: string;
  /// Leads the phone bar with Back instead of the drawer trigger.
  back?: BarBack;
  children?: ReactNode;
  className?: string;
};

export default function PageBar({ title, subtitle, back, children, className = '' }: PageBarProps) {
  return (
    <header className={`page-bar ${className}`.trim()}>
      <BarLead back={back} />
      <h1 className="page-bar-title">{title}</h1>
      {subtitle && <p className="page-bar-subtitle">{subtitle}</p>}
      {children && <div className="page-bar-actions">{children}</div>}
    </header>
  );
}
