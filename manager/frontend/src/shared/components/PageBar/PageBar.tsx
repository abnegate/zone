import type { ReactNode, Ref } from 'react';

type PageBarProps = {
  title: string;
  subtitle?: string;
  children?: ReactNode;
  className?: string;
  titleRef?: Ref<HTMLHeadingElement>;
};

export default function PageBar({
  title,
  subtitle,
  children,
  className = '',
  titleRef,
}: PageBarProps) {
  return (
    <header className={`page-bar ${className}`.trim()}>
      <h1 ref={titleRef} className="page-bar-title" tabIndex={titleRef ? -1 : undefined}>
        {title}
      </h1>
      {subtitle && <p className="page-bar-subtitle">{subtitle}</p>}
      {children && <div className="page-bar-actions">{children}</div>}
    </header>
  );
}
