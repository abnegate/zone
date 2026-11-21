import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import { SettingsGroup, SettingsSection, settingsPath } from '../../settings/navigation/sections';

export const LICENSE_PATH = settingsPath(SettingsGroup.Server, SettingsSection.License);

export const LICENSE_LINK = 'Go to License';

interface UpgradeNoticeProps {
  message: string;
  className: string;
  action?: string;
  announced?: boolean;
}

export function UpgradeNotice({
  message,
  className,
  action = LICENSE_LINK,
  announced = false,
}: UpgradeNoticeProps) {
  return (
    <div className={`settings-card ${className}`} role={announced ? 'alert' : 'note'}>
      <p>{message}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        {action}
      </Link>
    </div>
  );
}
