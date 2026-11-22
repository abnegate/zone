import { LicenseLink } from './LicenseLink';

export const RENEW_LINK = 'Renew on the License page';

export const ASK_FOR_RENEWAL = 'Ask your server admin to renew the license.';

interface LapsedNoticeProps {
  message: string;
  className: string;
}

export function LapsedNotice({ message, className }: LapsedNoticeProps) {
  return (
    <div className={`alert alert-warning ${className}`} role="status">
      <p>{message}</p>
      <LicenseLink label={RENEW_LINK} request={ASK_FOR_RENEWAL} />
    </div>
  );
}
