import {
  CREATED_LABEL,
  LAST_USED_LABEL,
  META_SEPARATOR,
  NEVER_USED,
  ROTATED_LABEL,
} from '../labels';
import type { TokenResponse } from '../types';
import { Moment } from './Moment';

interface ScimTokenMetaProps {
  token: TokenResponse;
}

export function ScimTokenMeta({ token }: ScimTokenMetaProps) {
  return (
    <span className="scim-item-meta">
      {CREATED_LABEL} <Moment value={token.created_at} />
      {META_SEPARATOR}
      {token.last_used_at === null ? (
        NEVER_USED
      ) : (
        <>
          {LAST_USED_LABEL} <Moment value={token.last_used_at} />
        </>
      )}
      {token.rotated_at !== null && (
        <>
          {META_SEPARATOR}
          {ROTATED_LABEL} <Moment value={token.rotated_at} />
        </>
      )}
    </span>
  );
}
