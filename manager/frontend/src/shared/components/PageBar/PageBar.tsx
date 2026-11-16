import type { ReactNode, Ref } from 'react';
import BarLead, { type BarBack } from './BarLead';

type PageBarProps = {
  title: string;
  subtitle?: string;
  /// Leads the phone bar with Back instead of the drawer trigger.
  back?: BarBack;
  children?: ReactNode;
  className?: string;
  titleRef?: Ref<HTMLHeadingElement>;
};

export default function PageBar({
  title,
  subtitle,
  back,
  children,
  className = '',
  titleRef,
}: PageBarProps) {
  return (
    <header className={`page-bar ${className}`.trim()}>
      <BarLead back={back} />
      <h1 ref={titleRef} className="page-bar-title" tabIndex={titleRef ? -1 : undefined}>
        {title}
      </h1>
      {subtitle && <p className="page-bar-subtitle">{subtitle}</p>}
      {children && <div className="page-bar-actions">{children}</div>}
    </header>
  );
}
