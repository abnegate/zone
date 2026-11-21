import { Link } from 'react-router-dom';
import { SettingsGroup, SettingsSection, settingsPath } from '../../settings/navigation/sections';
import { useAdministersServer } from './AdministersServer';

export const LICENSE_PATH = settingsPath(SettingsGroup.Server, SettingsSection.License);

interface LicenseLinkProps {
  label: string;
  request: string;
  className?: string;
}

export function LicenseLink({ label, request, className }: LicenseLinkProps) {
  if (!useAdministersServer()) {
    return <p>{request}</p>;
  }
  return (
    <Link to={LICENSE_PATH} className={className}>
      {label}
    </Link>
  );
}
