import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../licensePath';
import { useAdministersServer } from './AdministersServer';

interface LicenseLinkProps {
  label: string;
  request: string;
  className?: string;
}

export function LicenseLink({ label, request, className }: LicenseLinkProps) {
  if (!useAdministersServer()) {
    return <p>{request}</p>;
  }
  return (
    <Link to={LICENSE_PATH} className={className}>
      {label}
    </Link>
  );
}
