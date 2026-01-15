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
export function EmptyState({
  icon,
  title,
  description,
  action,
  className = '',
}: EmptyStateProps) {
  return (
    <div
      className={`flex flex-col items-center justify-center py-16 text-center ${className}`}
    >
      {icon && (
        <div className="text-muted-foreground/50 mb-4">{icon}</div>
      )}
      <h3 className="text-lg font-medium text-foreground mb-1">{title}</h3>
      {description && (
        <p className="text-muted-foreground mb-4">{description}</p>
      )}
      {action}
    </div>
  );
}
