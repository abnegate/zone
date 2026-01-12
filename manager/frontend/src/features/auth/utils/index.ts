/**
 * Validates the format of a token from URL parameters.
 * Tokens should be alphanumeric with optional hyphens/underscores and 20-200 chars long.
 */
export const isValidTokenFormat = (token: string | null): boolean => {
  if (!token) return false;
  return /^[a-zA-Z0-9_-]{20,200}$/.test(token);
};

// Parse user agent to friendly device/browser name
export function parseUserAgent(userAgent: string | null): string {
  if (!userAgent) return 'Unknown Device';

  // Handle empty or invalid strings
  if (typeof userAgent !== 'string' || userAgent.trim() === '') {
    return 'Unknown Device';
  }

  const ua = userAgent.toLowerCase();

  // Mobile detection first
  const isMobile = ua.includes('mobile') || ua.includes('android') || ua.includes('iphone');

  // Browser detection (order matters - more specific first)
  let browser = 'Unknown Browser';
  if (ua.includes('edg/') || ua.includes('edge/')) browser = 'Edge';
  else if (ua.includes('opr/') || ua.includes('opera/')) browser = 'Opera';
  else if (ua.includes('firefox/')) browser = 'Firefox';
  else if (ua.includes('chrome/')) browser = 'Chrome';
  else if (ua.includes('safari/')) browser = 'Safari';

  // OS detection (order matters - more specific first)
  let os = 'Unknown OS';
  if (ua.includes('iphone')) os = 'iOS (iPhone)';
  else if (ua.includes('ipad')) os = 'iOS (iPad)';
  else if (ua.includes('android')) os = 'Android';
  else if (ua.includes('windows')) os = 'Windows';
  else if (ua.includes('mac os x') || ua.includes('macintosh')) os = 'MacOS';
  else if (ua.includes('linux')) os = 'Linux';

  const deviceType = isMobile && !ua.includes('ipad') ? 'Mobile ' : '';

  // Return "Unknown device" for completely unknown combinations
  if (browser === 'Unknown Browser' && os === 'Unknown OS') {
    return 'Unknown Device';
  }

  return `${deviceType}${browser} on ${os}`;
}
