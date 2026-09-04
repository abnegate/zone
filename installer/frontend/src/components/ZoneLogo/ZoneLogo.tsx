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
        viewBox="0 0 64 64"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className="shrink-0 text-primary"
        aria-hidden="true"
      >
        <path
          fill="currentColor"
          d="M17 10H42L30.267 21H8V19C8 14.029 12.029 10 17 10ZM47 10C54 10 57 18 51 24L17 54C10 54 7 46 13 40L47 10ZM47 54H22L33.733 43H56V45C56 49.971 51.971 54 47 54Z"
        />
      </svg>
      {showText && (
        <span className={`${textSizeMap[size]} font-semibold tracking-tight text-foreground`}>
          Zone
        </span>
      )}
    </div>
  );
}
