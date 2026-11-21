import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import {
  SettingsGroup,
  SettingsSection,
  settingsPath,
} from '../../../settings/navigation/sections';
import { UPGRADE_NOTICE } from '../labels';

export const LICENSE_PATH = settingsPath(SettingsGroup.Server, SettingsSection.License);

export const LICENSE_LINK = 'Go to License';

export function RolesUpgradeNotice() {
  return (
    <div className="settings-card roles-notice" role="note">
      <p>{UPGRADE_NOTICE}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        {LICENSE_LINK}
      </Link>
    </div>
  );
}
