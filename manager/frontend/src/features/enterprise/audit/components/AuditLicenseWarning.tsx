import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../../licensePath';

interface AuditLicenseWarningProps {
  message: string;
  action: string;
}

export function AuditLicenseWarning({ message, action }: AuditLicenseWarningProps) {
  return (
    <div className="alert alert-warning audit-license-warning" role="status">
      <p>{message}</p>
      <Link to={LICENSE_PATH}>{action}</Link>
    </div>
  );
}
