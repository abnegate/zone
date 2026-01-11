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
        viewBox="0 0 100 100"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className="zone-logo__icon"
        aria-hidden="true"
      >
        <defs>
          <linearGradient id="zone-logo-grad" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="#667eea" />
            <stop offset="100%" stopColor="#764ba2" />
          </linearGradient>
        </defs>
        <circle cx="50" cy="50" r="42" stroke="url(#zone-logo-grad)" strokeWidth="1.5" fill="none" />
        <circle cx="50" cy="50" r="24" stroke="url(#zone-logo-grad)" strokeWidth="2.5" fill="none" />
        <circle cx="50" cy="50" r="8" fill="url(#zone-logo-grad)" />
      </svg>
      {showText && <span className="zone-logo__text">Zone</span>}
    </div>
  );
}
