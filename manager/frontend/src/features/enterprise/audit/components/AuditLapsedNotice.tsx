import { Link } from 'react-router-dom';
import { RENEW_LINK_LABEL } from '../labels';
import { LICENSE_PATH } from './AuditUpgradeNotice';

interface AuditLapsedNoticeProps {
  message: string;
}

export function AuditLapsedNotice({ message }: AuditLapsedNoticeProps) {
  return (
    <div className="alert alert-warning audit-lapsed" role="status">
      <p>{message}</p>
      <Link to={LICENSE_PATH}>{RENEW_LINK_LABEL}</Link>
    </div>
  );
}
