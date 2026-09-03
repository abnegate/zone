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
          fillRule="evenodd"
          d="M32 4c15.464 0 28 12.536 28 28S47.464 60 32 60 4 47.464 4 32 16.536 4 32 4Zm-13 13h26v8L29 38h16v8H19v-8l16-13H19z"
        />
      </svg>
      {showText && <span className="zone-logo__text">Zone</span>}
    </div>
  );
}
