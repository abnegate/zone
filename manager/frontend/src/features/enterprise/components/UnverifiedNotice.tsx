import { LicenseLink } from './LicenseLink';

export const REPLACE_LINK = 'Replace or remove it on the License page';

export const ASK_FOR_REPLACEMENT = 'Ask your server admin to replace or remove the license key.';

interface UnverifiedNoticeProps {
  message: string;
  className: string;
}

export function UnverifiedNotice({ message, className }: UnverifiedNoticeProps) {
  return (
    <div className={`alert alert-warning ${className}`} role="status">
      <p>{message}</p>
      <LicenseLink label={REPLACE_LINK} request={ASK_FOR_REPLACEMENT} />
    </div>
  );
}
