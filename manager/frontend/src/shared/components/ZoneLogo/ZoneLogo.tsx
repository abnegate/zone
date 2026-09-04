import './ZoneLogo.css';

interface ZoneLogoProps {
  size?: 'sm' | 'md' | 'lg' | 'xl';
  showText?: boolean;
  className?: string;
}

export default function ZoneLogo({ size = 'md', showText = true, className = '' }: ZoneLogoProps) {
  const sizeMap = {
    sm: 24,
    md: 32,
    lg: 48,
    xl: 64,
  };

  const iconSize = sizeMap[size];

  return (
    <div className={`zone-logo zone-logo--${size} ${className}`}>
      <svg
        width={iconSize}
        height={iconSize}
        viewBox="0 0 64 64"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className="zone-logo__icon"
        aria-hidden="true"
      >
        <path
          fill="currentColor"
          d="M17 10H42L30.267 21H8V19C8 14.029 12.029 10 17 10ZM47 10C54 10 57 18 51 24L17 54C10 54 7 46 13 40L47 10ZM47 54H22L33.733 43H56V45C56 49.971 51.971 54 47 54Z"
        />
      </svg>
      {showText && <span className="zone-logo__text">Zone</span>}
    </div>
  );
}
