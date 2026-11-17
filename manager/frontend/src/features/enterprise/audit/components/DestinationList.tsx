import { Badge, type BadgeProps, Button } from '@zone/ui';
import { useState } from 'react';
import { absoluteTime, relativeTime } from '../../../../shared/utils/time';
import { Switch } from '../../sso/components/Switch';
import { Tone } from '../../status';
import { useTestAuditDestination, useUpdateAuditDestination } from '../hooks';
import {
  ADD_DESTINATION_LABEL,
  BACKLOG_LABEL,
  DELETE_LABEL,
  DELIVERED_COUNT_LABEL,
  DESTINATIONS_DESCRIPTION,
  DESTINATIONS_TITLE,
  EDIT_LABEL,
  ENABLED_LABEL,
  LAST_ERROR_LABEL,
  LAST_SUCCESS_LABEL,
  NEVER_DELIVERED,
  NEXT_ATTEMPT_LABEL,
  NO_DESTINATIONS,
  NO_DESTINATIONS_READ_ONLY,
  S3_SCHEME,
  TEST_LABEL,
  UNENCRYPTED_LABEL,
} from '../labels';
import {
  backlogText,
  deliveryError,
  healthLabel,
  healthTone,
  kindLabel,
  protocolLabel,
  refusalMessage,
  stalledMessage,
  testResultMessage,
  unencryptedWarning,
} from '../messages';
import { AuditKind, DeliveryHealth } from '../schemas';
import type {
  AuditScope,
  DestinationResponse,
  DestinationsResponse,
  Settings,
  TestResponse,
} from '../types';
import { DeleteDestinationDialog } from './DeleteDestinationDialog';
import { DestinationForm } from './DestinationForm';

const BADGE_VARIANTS: Readonly<Record<Tone, BadgeProps['variant']>> = {
  [Tone.Neutral]: 'neutral',
  [Tone.Success]: 'success',
  [Tone.Warning]: 'warning',
  [Tone.Error]: 'destructive',
};

const SEPARATOR = ' · ';

function target(settings: Settings): string {
  switch (settings.kind) {
    case AuditKind.Webhook:
      return settings.url;
    case AuditKind.S3:
      return [
        `${S3_SCHEME}${settings.bucket}/${settings.prefix}`,
        settings.region,
        settings.endpoint,
      ]
        .filter((part) => part !== null && part !== '')
        .join(SEPARATOR);
    case AuditKind.Syslog:
      return `${settings.host}:${settings.port}${SEPARATOR}${protocolLabel(settings.protocol)}`;
  }
}

function Moment({ value }: { value: string }) {
  return (
    <time dateTime={value} title={absoluteTime(value)}>
      {relativeTime(value)}
    </time>
  );
}

interface DestinationRowProps {
  destination: DestinationResponse;
  streaming: boolean;
  toggling: boolean;
  testing: boolean;
  result: TestResponse | null;
  failure: string | null;
  onToggle(enabled: boolean): void;
  onTest(): void;
  onEdit(): void;
  onDelete(): void;
}

function DestinationRow({
  destination,
  streaming,
  toggling,
  testing,
  result,
  failure,
  onToggle,
  onTest,
  onEdit,
  onDelete,
}: DestinationRowProps) {
  const { delivery } = destination;
  const warning = unencryptedWarning(destination.settings);
  const error = delivery.health === DeliveryHealth.Healthy ? null : deliveryError(delivery);
  const retrying = delivery.health === DeliveryHealth.Failing && delivery.next_attempt_at !== null;

  return (
    <li className="audit-destination" aria-label={destination.name}>
      <div className="audit-destination-identity">
        <div className="audit-destination-heading">
          <span className="audit-destination-name">{destination.name}</span>
          <Badge variant="outline">{kindLabel(destination.kind)}</Badge>
          <Badge variant={BADGE_VARIANTS[healthTone(delivery.health)]}>
            {healthLabel(delivery.health)}
          </Badge>
          {warning && (
            <Badge variant="warning" title={warning}>
              {UNENCRYPTED_LABEL}
            </Badge>
          )}
        </div>
        <span className="audit-destination-target">{target(destination.settings)}</span>
        <dl className="audit-stats">
          <div className="audit-stat">
            <dt>{BACKLOG_LABEL}</dt>
            <dd>{backlogText(delivery.backlog)}</dd>
          </div>
          <div className="audit-stat">
            <dt>{LAST_SUCCESS_LABEL}</dt>
            <dd>
              {delivery.last_success_at ? (
                <Moment value={delivery.last_success_at} />
              ) : (
                NEVER_DELIVERED
              )}
            </dd>
          </div>
          <div className="audit-stat">
            <dt>{DELIVERED_COUNT_LABEL}</dt>
            <dd>{delivery.delivered_count.toLocaleString()}</dd>
          </div>
          {retrying && delivery.next_attempt_at && (
            <div className="audit-stat">
              <dt>{NEXT_ATTEMPT_LABEL}</dt>
              <dd>
                <Moment value={delivery.next_attempt_at} />
              </dd>
            </div>
          )}
        </dl>
        {error && (
          <p className="audit-destination-error">
            <span className="audit-destination-error-label">{LAST_ERROR_LABEL}</span>{' '}
            {delivery.last_failure_at && (
              <>
                <Moment value={delivery.last_failure_at} />:{' '}
              </>
            )}
            {error}
          </p>
        )}
        {result && (
          <p
            className={
              result.delivered ? 'audit-test-result' : 'audit-test-result audit-test-failed'
            }
            role="status"
          >
            {testResultMessage(result)}
          </p>
        )}
        {failure && (
          <p className="audit-row-error" role="alert">
            {failure}
          </p>
        )}
      </div>
      <div className="audit-destination-controls">
        <Switch
          label={ENABLED_LABEL}
          checked={destination.enabled}
          disabled={toggling || (!streaming && !destination.enabled)}
          onChange={onToggle}
        />
        <div className="audit-destination-actions">
          {streaming && (
            <>
              <Button type="button" variant="ghost" size="sm" loading={testing} onClick={onTest}>
                {TEST_LABEL}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                aria-label={`${EDIT_LABEL} ${destination.name}`}
                onClick={onEdit}
              >
                {EDIT_LABEL}
              </Button>
            </>
          )}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={`${DELETE_LABEL} ${destination.name}`}
            onClick={onDelete}
          >
            {DELETE_LABEL}
          </Button>
        </div>
      </div>
    </li>
  );
}

interface DestinationListProps {
  scope: AuditScope;
  response: DestinationsResponse;
}

export function DestinationList({ scope, response }: DestinationListProps) {
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<DestinationResponse | null>(null);
  const [deleting, setDeleting] = useState<DestinationResponse | null>(null);
  const [toggled, setToggled] = useState<string | null>(null);
  const update = useUpdateAuditDestination(scope);
  const test = useTestAuditDestination(scope);
  const { standing, destinations, stalled_since: stalledSince } = response;
  const { streaming } = standing;

  const toggleFailure = update.refusal ? refusalMessage(update.refusal.code) : update.error;
  const testFailure = test.refusal ? refusalMessage(test.refusal.code) : test.error;

  const toggle = (destination: DestinationResponse, enabled: boolean) => {
    setToggled(destination.id);
    update.update(destination.id, { enabled });
  };

  const run = (destination: DestinationResponse) => {
    test.reset();
    test.test(destination.id);
  };

  return (
    <div className="settings-card audit-card audit-destinations">
      <div className="audit-card-header">
        <div className="audit-card-copy">
          <h3 className="audit-card-title">{DESTINATIONS_TITLE}</h3>
          <p className="audit-hint">{DESTINATIONS_DESCRIPTION}</p>
        </div>
        {streaming && (
          <Button type="button" size="sm" onClick={() => setAdding(true)}>
            {ADD_DESTINATION_LABEL}
          </Button>
        )}
      </div>
      {stalledSince && (
        <div className="alert alert-warning audit-stalled" role="status">
          {stalledMessage(stalledSince)}
        </div>
      )}
      {destinations.length === 0 ? (
        <p className="audit-hint">{streaming ? NO_DESTINATIONS : NO_DESTINATIONS_READ_ONLY}</p>
      ) : (
        <ul className="audit-destination-list">
          {destinations.map((destination) => {
            const tested = test.testing === destination.id;
            return (
              <DestinationRow
                key={destination.id}
                destination={destination}
                streaming={streaming}
                toggling={update.pending && toggled === destination.id}
                testing={test.pending && tested}
                result={tested ? (test.result ?? null) : null}
                failure={
                  (tested ? testFailure : null) ??
                  (toggled === destination.id ? toggleFailure : null)
                }
                onToggle={(enabled) => toggle(destination, enabled)}
                onTest={() => run(destination)}
                onEdit={() => setEditing(destination)}
                onDelete={() => setDeleting(destination)}
              />
            );
          })}
        </ul>
      )}
      {adding && <DestinationForm scope={scope} onClose={() => setAdding(false)} />}
      {editing && (
        <DestinationForm
          key={editing.id}
          scope={scope}
          destination={editing}
          onClose={() => setEditing(null)}
        />
      )}
      {deleting && (
        <DeleteDestinationDialog
          scope={scope}
          destination={deleting}
          onClose={() => setDeleting(null)}
        />
      )}
    </div>
  );
}
