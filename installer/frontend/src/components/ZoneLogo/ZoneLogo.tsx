interface ZoneLogoProps {
  size?: 'sm' | 'md' | 'lg';
  showText?: boolean;
}

export default function ZoneLogo({ size = 'md', showText = true }: ZoneLogoProps) {
  const sizeMap = {
    sm: 24,
    md: 32,
    lg: 48,
  };
  const textSizeMap = {
    sm: 'text-base',
    md: 'text-lg',
    lg: 'text-2xl',
  };

  const iconSize = sizeMap[size];

  return (
    <div className="flex items-center gap-2">
      <svg
        width={iconSize}
        height={iconSize}
        viewBox="0 0 100 100"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className="shrink-0 text-foreground"
        aria-hidden="true"
      >
        <circle cx="50" cy="50" r="42" stroke="currentColor" strokeWidth="1.5" fill="none" opacity="0.45" />
        <circle cx="50" cy="50" r="24" stroke="currentColor" strokeWidth="2.5" fill="none" opacity="0.75" />
        <circle cx="50" cy="50" r="8" fill="currentColor" />
      </svg>
      {showText && (
        <span className={`${textSizeMap[size]} font-semibold tracking-tight text-foreground`}>
          Zone
        </span>
      )}
    </div>
  );
}
