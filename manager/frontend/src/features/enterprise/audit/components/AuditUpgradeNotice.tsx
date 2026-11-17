import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import {
  SettingsGroup,
  SettingsSection,
  settingsPath,
} from '../../../settings/navigation/sections';
import { LICENSE_LINK_LABEL } from '../labels';

export const LICENSE_PATH = settingsPath(SettingsGroup.Server, SettingsSection.License);

interface AuditUpgradeNoticeProps {
  message: string;
}

export function AuditUpgradeNotice({ message }: AuditUpgradeNoticeProps) {
  return (
    <div className="settings-card audit-notice" role="note">
      <p>{message}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        {LICENSE_LINK_LABEL}
      </Link>
    </div>
  );
}
