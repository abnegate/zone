import { Navigate, useLocation } from 'react-router-dom';
import { legacyOrganizationPath, relocate, SESSIONS_PATH } from './legacy';

export const LegacySettings = {
  Organization: 'organization',
  Sessions: 'sessions',
} as const;

export type LegacySettings = (typeof LegacySettings)[keyof typeof LegacySettings];

export default function SettingsRedirect({ from }: { from: LegacySettings }) {
  const location = useLocation();
  const target =
    from === LegacySettings.Organization
      ? legacyOrganizationPath(location.search, location.hash)
      : relocate(SESSIONS_PATH, location.search, location.hash);
  return <Navigate to={target} replace />;
}
