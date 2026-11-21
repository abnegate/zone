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
          d="M12.954 53.72L44.13 8H20A12 12 0 0 0 8 20V44A12 12 0 0 0 12.954 53.72ZM51.046 10.28L19.87 56H44A12 12 0 0 0 56 44V20A12 12 0 0 0 51.046 10.28Z"
        />
      </svg>
      {showText && <span className="zone-logo__text">Zone</span>}
    </div>
  );
}
