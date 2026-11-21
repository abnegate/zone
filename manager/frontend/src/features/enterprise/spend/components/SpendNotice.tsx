import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../../audit/components';
import {
  LAPSED_ACTION,
  LAPSED_NOTICE,
  UPGRADE_ACTION,
  UPGRADE_DETAIL,
  UPGRADE_NOTICE,
} from '../labels';

export const SpendNoticeVariant = {
  Upgrade: 'upgrade',
  Lapsed: 'lapsed',
} as const;

export type SpendNoticeVariant = (typeof SpendNoticeVariant)[keyof typeof SpendNoticeVariant];

interface SpendNoticeProps {
  variant: SpendNoticeVariant;
}

export function SpendNotice({ variant }: SpendNoticeProps) {
  if (variant === SpendNoticeVariant.Lapsed) {
    return (
      <div className="alert alert-warning spend-notice" role="status">
        <p>{LAPSED_NOTICE}</p>
        <Link to={LICENSE_PATH}>{LAPSED_ACTION}</Link>
      </div>
    );
  }
  return (
    <div className="settings-card spend-notice" role="note">
      <p className="spend-notice-title">{UPGRADE_NOTICE}</p>
      <p>{UPGRADE_DETAIL}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        {UPGRADE_ACTION}
      </Link>
    </div>
  );
}
