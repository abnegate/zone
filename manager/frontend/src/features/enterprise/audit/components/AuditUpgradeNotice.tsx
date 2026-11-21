import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../../licensePath';
import { LICENSE_LINK_LABEL } from '../labels';

interface AuditUpgradeNoticeProps {
  message: string;
  action?: string;
}

export function AuditUpgradeNotice({
  message,
  action = LICENSE_LINK_LABEL,
}: AuditUpgradeNoticeProps) {
  return (
    <div className="settings-card audit-notice" role="note">
      <p>{message}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        {action}
      </Link>
    </div>
  );
}
