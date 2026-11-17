import { Button, Input, Label, Select } from '@zone/ui';
import { useId } from 'react';
import { Switch } from '../../components/Switch';
import {
  clearable,
  type DestinationDraft,
  draftSecrets,
  generateSecret,
  removesCertificates,
  settingsOf,
  withProtocol,
} from '../draft';
import {
  ADDRESSING_LABEL,
  BUCKET_LABEL,
  BUCKET_PLACEHOLDER,
  CERTIFICATES_HINT,
  CERTIFICATES_REMOVED,
  ENABLED_DESCRIPTION,
  ENABLED_LABEL,
  ENDPOINT_HINT,
  ENDPOINT_LABEL,
  ENDPOINT_PLACEHOLDER,
  FACILITY_LABEL,
  HOST_LABEL,
  HOST_PLACEHOLDER,
  HOSTNAME_LABEL,
  KIND_LABEL,
  NAME_LABEL,
  NAME_PLACEHOLDER,
  PORT_LABEL,
  PREFIX_LABEL,
  PREFIX_PLACEHOLDER,
  PROTOCOL_LABEL,
  REGION_LABEL,
  REGION_PLACEHOLDER,
  SECRET_CLEAR_LABEL,
  SECRET_KEEP_LABEL,
  SECRET_REMOVED,
  SECRET_SAVED_PLACEHOLDER,
  SIGNATURE_HINT,
  URL_LABEL,
  URL_PLACEHOLDER,
} from '../labels';
import {
  addressingLabel,
  DestinationField,
  facilityLabel,
  kindLabel,
  protocolLabel,
  secretLabel,
  unencryptedWarning,
} from '../messages';
import {
  AUDIT_KINDS,
  AuditKind,
  FACILITY_MAXIMUM,
  FACILITY_MINIMUM,
  S3_ADDRESSINGS,
  SecretName,
  SYSLOG_PROTOCOLS,
} from '../schemas';
import type {
  AuditKind as AuditKindValue,
  S3Addressing,
  SecretName as SecretNameValue,
  SyslogProtocol,
} from '../types';

const KIND_OPTIONS = AUDIT_KINDS.map((kind) => ({ value: kind, label: kindLabel(kind) }));

const PROTOCOL_OPTIONS = SYSLOG_PROTOCOLS.map((protocol) => ({
  value: protocol,
  label: protocolLabel(protocol),
}));

const ADDRESSING_OPTIONS = S3_ADDRESSINGS.map((addressing) => ({
  value: addressing,
  label: addressingLabel(addressing),
}));

const FACILITY_OPTIONS = Array.from(
  { length: FACILITY_MAXIMUM - FACILITY_MINIMUM + 1 },
  (_, index) => {
    const facility = FACILITY_MINIMUM + index;
    return { value: String(facility), label: facilityLabel(facility) };
  }
);

const MASKED: ReadonlySet<SecretNameValue> = new Set([
  SecretName.AccessKeyId,
  SecretName.SecretAccessKey,
]);

const CERTIFICATE_ROWS = 4;

export type FieldError = (field: DestinationField | SecretNameValue) => string | undefined;

interface DestinationFieldsProps {
  draft: DestinationDraft;
  saved: readonly SecretNameValue[];
  editing: boolean;
  disabled: boolean;
  error: FieldError;
  onChange(draft: DestinationDraft): void;
}

interface SecretFieldProps {
  id: string;
  name: SecretNameValue;
  draft: DestinationDraft;
  saved: boolean;
  disabled: boolean;
  error: string | undefined;
  onChange(draft: DestinationDraft): void;
}

function SecretField({ id, name, draft, saved, disabled, error, onChange }: SecretFieldProps) {
  const value = draft.secrets[name];
  const removed = draft.cleared.includes(name);
  const placeholder = saved && !removed ? SECRET_SAVED_PLACEHOLDER : undefined;
  const type = (secret: string) =>
    onChange({ ...draft, secrets: { ...draft.secrets, [name]: secret } });
  const toggle = () =>
    onChange({
      ...draft,
      cleared: removed
        ? draft.cleared.filter((current) => current !== name)
        : [...draft.cleared, name],
    });

  if (clearable(name)) {
    const hintId = `${id}-hint`;
    const errorId = `${id}-error`;
    return (
      <div className="audit-secret">
        <div className="audit-secret-header">
          <Label htmlFor={id}>{secretLabel(name)}</Label>
          {saved && (
            <Button type="button" variant="ghost" size="sm" disabled={disabled} onClick={toggle}>
              {removed ? SECRET_KEEP_LABEL : SECRET_CLEAR_LABEL}
            </Button>
          )}
        </div>
        <textarea
          id={id}
          className="audit-certificate"
          rows={CERTIFICATE_ROWS}
          value={value}
          placeholder={placeholder}
          spellCheck={false}
          autoComplete="off"
          disabled={disabled}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? errorId : removed ? hintId : undefined}
          onChange={(event) => type(event.target.value)}
        />
        {error && (
          <p id={errorId} className="ui-input-error-text" role="alert">
            {error}
          </p>
        )}
        {removed && !error && (
          <p id={hintId} className="ui-input-help-text">
            {SECRET_REMOVED}
          </p>
        )}
      </div>
    );
  }

  const signing = name === SecretName.SigningSecret;
  return (
    <Input
      id={id}
      label={secretLabel(name)}
      type={MASKED.has(name) ? 'password' : 'text'}
      className={signing ? 'audit-mono' : undefined}
      value={value}
      placeholder={placeholder}
      autoComplete={MASKED.has(name) ? 'new-password' : 'off'}
      spellCheck={false}
      disabled={disabled}
      helpText={signing ? SIGNATURE_HINT : undefined}
      error={error}
      onGenerate={signing && !disabled ? () => type(generateSecret()) : undefined}
      onChange={(event) => type(event.target.value)}
    />
  );
}

export function DestinationFields({
  draft,
  saved,
  editing,
  disabled,
  error,
  onChange,
}: DestinationFieldsProps) {
  const base = useId();
  const set = <K extends keyof DestinationDraft>(key: K, value: DestinationDraft[K]) =>
    onChange({ ...draft, [key]: value });
  const warning = unencryptedWarning(settingsOf(draft));
  const secrets = draftSecrets(draft);

  return (
    <fieldset className="audit-fields" disabled={disabled}>
      <Input
        id={`${base}-name`}
        label={NAME_LABEL}
        value={draft.name}
        autoComplete="off"
        placeholder={NAME_PLACEHOLDER}
        error={error(DestinationField.Name)}
        onChange={(event) => set('name', event.target.value)}
      />
      <Select
        id={`${base}-kind`}
        label={KIND_LABEL}
        value={draft.kind}
        options={KIND_OPTIONS}
        disabled={disabled || editing}
        error={error(DestinationField.Kind)}
        onValueChange={(kind) => set('kind', kind as AuditKindValue)}
      />
      {draft.kind === AuditKind.Webhook && (
        <Input
          id={`${base}-url`}
          label={URL_LABEL}
          type="url"
          inputMode="url"
          className="audit-mono"
          value={draft.url}
          autoComplete="off"
          spellCheck={false}
          placeholder={URL_PLACEHOLDER}
          error={error(DestinationField.Url)}
          onChange={(event) => set('url', event.target.value)}
        />
      )}
      {draft.kind === AuditKind.S3 && (
        <>
          <div className="audit-field-row">
            <Input
              id={`${base}-bucket`}
              label={BUCKET_LABEL}
              value={draft.bucket}
              autoComplete="off"
              spellCheck={false}
              placeholder={BUCKET_PLACEHOLDER}
              error={error(DestinationField.Bucket)}
              onChange={(event) => set('bucket', event.target.value)}
            />
            <Input
              id={`${base}-region`}
              label={REGION_LABEL}
              value={draft.region}
              autoComplete="off"
              spellCheck={false}
              placeholder={REGION_PLACEHOLDER}
              error={error(DestinationField.Region)}
              onChange={(event) => set('region', event.target.value)}
            />
          </div>
          <Input
            id={`${base}-endpoint`}
            label={ENDPOINT_LABEL}
            type="url"
            inputMode="url"
            className="audit-mono"
            value={draft.endpoint}
            autoComplete="off"
            spellCheck={false}
            placeholder={ENDPOINT_PLACEHOLDER}
            helpText={ENDPOINT_HINT}
            error={error(DestinationField.Endpoint)}
            onChange={(event) => set('endpoint', event.target.value)}
          />
          <div className="audit-field-row">
            <Input
              id={`${base}-prefix`}
              label={PREFIX_LABEL}
              className="audit-mono"
              value={draft.prefix}
              autoComplete="off"
              spellCheck={false}
              placeholder={PREFIX_PLACEHOLDER}
              error={error(DestinationField.Prefix)}
              onChange={(event) => set('prefix', event.target.value)}
            />
            <Select
              id={`${base}-addressing`}
              label={ADDRESSING_LABEL}
              value={draft.addressing}
              options={ADDRESSING_OPTIONS}
              disabled={disabled}
              onValueChange={(addressing) => set('addressing', addressing as S3Addressing)}
            />
          </div>
        </>
      )}
      {draft.kind === AuditKind.Syslog && (
        <>
          <div className="audit-field-row">
            <Input
              id={`${base}-host`}
              label={HOST_LABEL}
              className="audit-mono"
              value={draft.host}
              autoComplete="off"
              spellCheck={false}
              placeholder={HOST_PLACEHOLDER}
              error={error(DestinationField.Host)}
              onChange={(event) => set('host', event.target.value)}
            />
            <Input
              id={`${base}-port`}
              label={PORT_LABEL}
              inputMode="numeric"
              value={draft.port}
              autoComplete="off"
              error={error(DestinationField.Port)}
              onChange={(event) => set('port', event.target.value)}
            />
          </div>
          <div className="audit-field-row">
            <Select
              id={`${base}-protocol`}
              label={PROTOCOL_LABEL}
              value={draft.protocol}
              options={PROTOCOL_OPTIONS}
              disabled={disabled}
              onValueChange={(protocol) =>
                onChange(withProtocol(draft, protocol as SyslogProtocol))
              }
            />
            <Select
              id={`${base}-facility`}
              label={FACILITY_LABEL}
              value={String(draft.facility)}
              options={FACILITY_OPTIONS}
              disabled={disabled}
              error={error(DestinationField.Facility)}
              onValueChange={(facility) => set('facility', Number(facility))}
            />
          </div>
          <Input
            id={`${base}-hostname`}
            label={HOSTNAME_LABEL}
            className="audit-mono"
            value={draft.hostname}
            autoComplete="off"
            spellCheck={false}
            error={error(DestinationField.Hostname)}
            onChange={(event) => set('hostname', event.target.value)}
          />
          {removesCertificates(draft, saved) && (
            <p className="audit-hint" role="status">
              {CERTIFICATES_REMOVED}
            </p>
          )}
        </>
      )}
      {warning && (
        <div className="audit-warning" role="status">
          {warning}
        </div>
      )}
      {secrets.length > 0 && (
        <div className="audit-secrets">
          {draft.kind === AuditKind.Syslog && <p className="audit-hint">{CERTIFICATES_HINT}</p>}
          {secrets.map((name) => (
            <SecretField
              key={name}
              id={`${base}-${name}`}
              name={name}
              draft={draft}
              saved={saved.includes(name)}
              disabled={disabled}
              error={error(name)}
              onChange={onChange}
            />
          ))}
        </div>
      )}
      {error(DestinationField.Secrets) && (
        <div className="modal-error" role="alert">
          {error(DestinationField.Secrets)}
        </div>
      )}
      <Switch
        label={ENABLED_LABEL}
        description={ENABLED_DESCRIPTION}
        checked={draft.enabled}
        disabled={disabled}
        onChange={(enabled) => set('enabled', enabled)}
      />
    </fieldset>
  );
}
