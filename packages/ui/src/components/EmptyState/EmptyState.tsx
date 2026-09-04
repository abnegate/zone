import type { ReactNode } from 'react';

export interface EmptyStateProps {
  /** Icon to display (SVG element or component) */
  icon?: ReactNode;
  /** Main heading text */
  title: string;
  /** Description text below the heading */
  description?: string;
  /** Call-to-action button or element */
  action?: ReactNode;
  /** Additional CSS classes */
  className?: string;
}

/**
 * EmptyState component for displaying placeholder content when a list or section is empty.
 * Provides a consistent layout with icon, title, description, and optional action button.
 */
export function EmptyState({ icon, title, description, action, className = '' }: EmptyStateProps) {
  return (
    <div className={`ui-empty ${className}`}>
      {icon && <div className="ui-empty-icon">{icon}</div>}
      <h3 className="ui-empty-title">{title}</h3>
      {description && <p className="ui-empty-description">{description}</p>}
      {action && <div className="ui-empty-action">{action}</div>}
    </div>
  );
}
