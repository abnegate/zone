import { Badge } from '@zone/ui';
import {
  ERRORS_TITLE,
  LAST_SYNC_LABEL,
  META_SEPARATOR,
  NEVER_SYNCED,
  NO_ERRORS,
  SYNC_TITLE,
} from '../labels';
import type { FailureResponse } from '../types';
import { Moment } from './Moment';

interface ScimErrorListProps {
  lastSyncedAt: string | null;
  errors: readonly FailureResponse[];
}

export function ScimErrorList({ lastSyncedAt, errors }: ScimErrorListProps) {
  return (
    <div className="settings-card scim-card">
      <h3 className="scim-card-title">{SYNC_TITLE}</h3>
      <p className="scim-sync">
        {lastSyncedAt === null ? (
          NEVER_SYNCED
        ) : (
          <>
            {LAST_SYNC_LABEL} <Moment value={lastSyncedAt} />
          </>
        )}
      </p>
      <h4 className="scim-subtitle">{ERRORS_TITLE}</h4>
      {errors.length === 0 ? (
        <p className="scim-hint">{NO_ERRORS}</p>
      ) : (
        <ul className="scim-list">
          {errors.map((failure) => (
            <li key={failure.id} className="scim-failure">
              <div className="scim-item-heading">
                <Badge variant="destructive">{failure.status}</Badge>
                <code className="scim-request">
                  {failure.method} {failure.path}
                </code>
              </div>
              <span className="scim-failure-detail">{failure.detail}</span>
              <span className="scim-item-meta">
                <Moment value={failure.occurred_at} />
                {failure.scim_type !== null && (
                  <>
                    {META_SEPARATOR}
                    {failure.scim_type}
                  </>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
