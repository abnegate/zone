import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import {
  SettingsGroup,
  SettingsSection,
  settingsPath,
} from '../../../settings/navigation/sections';
import { UPGRADE_NOTICE } from '../labels';

export const LICENSE_PATH = settingsPath(SettingsGroup.Server, SettingsSection.License);

export function PolicyUpgradeNotice() {
  return (
    <div className="settings-card policy-notice" role="note">
      <p>{UPGRADE_NOTICE}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        Go to License
      </Link>
    </div>
  );
}
