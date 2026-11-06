import { Navigate, useLocation } from 'react-router-dom';
import { legacyOrganizationPath, SESSIONS_PATH } from './legacy';

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
      : SESSIONS_PATH;
  return <Navigate to={target} replace />;
}
