import { Button } from '@zone/ui';
import {
  CREATE_LABEL,
  hintLabel,
  NO_TOKENS_EDITABLE,
  NO_TOKENS_READ_ONLY,
  REVOKE_LABEL,
  ROTATE_LABEL,
  TOKENS_TITLE,
} from '../labels';
import type { TokenResponse } from '../types';
import { ScimTokenMeta } from './ScimTokenMeta';

interface ScimTokenListProps {
  tokens: readonly TokenResponse[];
  editable: boolean;
  onCreate(): void;
  onRotate(token: TokenResponse): void;
  onRevoke(token: TokenResponse): void;
}

export function ScimTokenList({
  tokens,
  editable,
  onCreate,
  onRotate,
  onRevoke,
}: ScimTokenListProps) {
  return (
    <div className="settings-card scim-card">
      <div className="scim-card-header">
        <h3 className="scim-card-title">{TOKENS_TITLE}</h3>
        {editable && (
          <Button type="button" size="sm" onClick={onCreate}>
            {CREATE_LABEL}
          </Button>
        )}
      </div>
      {tokens.length === 0 ? (
        <p className="scim-hint">{editable ? NO_TOKENS_EDITABLE : NO_TOKENS_READ_ONLY}</p>
      ) : (
        <ul className="scim-list">
          {tokens.map((token) => (
            <li key={token.id} className="scim-item">
              <div className="scim-item-identity">
                <div className="scim-item-heading">
                  <span className="scim-item-name">{token.name}</span>
                  <code className="scim-token-hint">{hintLabel(token.hint)}</code>
                </div>
                <ScimTokenMeta token={token} />
              </div>
              <div className="scim-item-actions">
                {editable && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label={`${ROTATE_LABEL} ${token.name}`}
                    onClick={() => onRotate(token)}
                  >
                    {ROTATE_LABEL}
                  </Button>
                )}
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  aria-label={`${REVOKE_LABEL} ${token.name}`}
                  onClick={() => onRevoke(token)}
                >
                  {REVOKE_LABEL}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
