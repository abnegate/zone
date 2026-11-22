import { LicenseLink } from '../../components/LicenseLink';

interface AuditLicenseWarningProps {
  message: string;
  action: string;
  request: string;
}

export function AuditLicenseWarning({ message, action, request }: AuditLicenseWarningProps) {
  return (
    <div className="alert alert-warning audit-license-warning" role="status">
      <p>{message}</p>
      <LicenseLink label={action} request={request} />
    </div>
  );
}
