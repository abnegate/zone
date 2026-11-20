import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import {
  SettingsGroup,
  SettingsSection,
  settingsPath,
} from '../../../settings/navigation/sections';

export const UPGRADE_NOTICE =
  'Single sign-on is part of Zone Enterprise. Add a license to let people sign in with Okta, Microsoft Entra ID or Google Workspace.';

export const LICENSE_PATH = settingsPath(SettingsGroup.Server, SettingsSection.License);

export function SsoUpgradeNotice() {
  return (
    <div className="settings-card sso-notice" role="note">
      <p>{UPGRADE_NOTICE}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        Go to License
      </Link>
    </div>
  );
}
