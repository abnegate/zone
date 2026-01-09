/**
 * Validates the format of a token from URL parameters.
 * Tokens should be alphanumeric with optional hyphens/underscores and 20-200 chars long.
 */
export const isValidTokenFormat = (token: string | null): boolean => {
  if (!token) return false;
  return /^[a-zA-Z0-9_-]{20,200}$/.test(token);
};
