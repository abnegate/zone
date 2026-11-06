export interface MenuIconProps {
  path: string;
  className?: string;
}

export function MenuIcon({ path, className }: MenuIconProps) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      aria-hidden="true"
    >
      <path d={path} />
    </svg>
  );
}
