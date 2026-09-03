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
        <circle cx="50" cy="50" r="42" stroke="currentColor" strokeWidth="1.5" fill="none" />
        <circle cx="50" cy="50" r="24" stroke="currentColor" strokeWidth="2.5" fill="none" />
        <circle cx="50" cy="50" r="8" fill="currentColor" />
      </svg>
      {showText && <span className="zone-logo__text">Zone</span>}
    </div>
  );
}
