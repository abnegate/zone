import type { ReactNode } from 'react';

type PageBarProps = {
  title: string;
  subtitle?: string;
  children?: ReactNode;
  className?: string;
};

export default function PageBar({ title, subtitle, children, className = '' }: PageBarProps) {
  return (
    <header className={`page-bar ${className}`.trim()}>
      <h1 className="page-bar-title">{title}</h1>
      {subtitle && <p className="page-bar-subtitle">{subtitle}</p>}
      {children && <div className="page-bar-actions">{children}</div>}
    </header>
  );
}
