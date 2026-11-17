export const SECTION_TITLE = 'Audit export';

export const ORGANIZATION_DESCRIPTION =
  "Stream this organization's audit log to your SIEM, decide how long events are kept, and download it for a date range.";

export const SERVER_DESCRIPTION =
  'Stream server-wide events like sign-ins and license changes, decide how long they are kept, and download them.';

export const AUDIT_LOG_LINK = 'Export, streaming and retention';

export const UPGRADE_NOTICE =
  'Audit export is part of Zone Team and Enterprise. Add a license to download the audit log, and Enterprise to stream it to a webhook, S3 or syslog and set a retention period.';

export const EDITION_NOTICE =
  'Your Team license covers downloading the audit log. Streaming to a SIEM and retention need Zone Enterprise, so destinations stay paused until you upgrade.';

export const ENTERPRISE_NOTICE =
  'Streaming to a SIEM and retention need Zone Enterprise, so destinations stay paused until you upgrade.';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Streaming is paused and its backlog is kept until you renew; the retention period already set is still applied, and downloads are unavailable.';

export const LAPSED_TEAM_NOTICE =
  'Your Zone Team license has lapsed, so downloads are unavailable until you renew. The retention period already set is still applied. Streaming to a SIEM and changing retention also need Zone Enterprise.';

export const UNVERIFIED_NOTICE =
  "Zone can't verify the stored license key, so streaming is paused and downloads are unavailable. The retention period already set is still applied. Paste the license key again to resume.";

export const READ_ONLY_UNLICENSED =
  'You can see these settings, but changing them needs a Zone Enterprise license.';

export const READ_ONLY_EDITION =
  "Changing these settings needs Zone Enterprise; a Team license doesn't include them.";

export const READ_ONLY_LAPSED = 'Renew your Zone license to change these settings.';

export const READ_ONLY_UNVERIFIED = 'Paste the license key again to change these settings.';

export const DOWNLOAD_READ_ONLY = 'Downloads need an active Team or Enterprise license.';

export const DESTINATIONS_TITLE = 'Streaming destinations';

export const DESTINATIONS_DESCRIPTION =
  'Every audit event is sent at least once to each enabled destination. Receivers can drop duplicates by the event id. A new destination starts with events written after it is added; download earlier history below.';

export const NO_DESTINATIONS =
  'No destinations yet. Add a webhook, an S3 bucket or a syslog server.';

export const ADD_DESTINATION_LABEL = 'Add destination';

export const EDIT_DESTINATION_TITLE = 'Edit destination';

export const NEW_DESTINATION_TITLE = 'Add destination';

export const SAVE_DESTINATION_LABEL = 'Save destination';

export const CANCEL_LABEL = 'Cancel';

export const EDIT_LABEL = 'Edit';

export const DELETE_LABEL = 'Delete';

export const TEST_LABEL = 'Send test event';

export const TEST_DESTINATION_LABEL = `${TEST_LABEL} to {}`;

export const ENABLED_LABEL = 'Enabled';

export const ENABLED_DESTINATION_LABEL = `${ENABLED_LABEL} for {}`;

export const DELETE_TITLE = 'Delete destination';

export const DELETE_CONFIRM_LABEL = 'Delete destination';

export const NAME_LABEL = 'Name';

export const KIND_LABEL = 'Type';

export const URL_LABEL = 'URL';

export const BUCKET_LABEL = 'Bucket';

export const REGION_LABEL = 'Region';

export const ENDPOINT_LABEL = 'Endpoint';

export const ENDPOINT_HINT =
  'Leave blank for AWS. Enter the URL of MinIO, R2 or another S3-compatible store.';

export const PREFIX_LABEL = 'Key prefix';

export const ADDRESSING_LABEL = 'Addressing';

export const HOST_LABEL = 'Host';

export const PORT_LABEL = 'Port';

export const PROTOCOL_LABEL = 'Protocol';

export const FACILITY_LABEL = 'Facility';

export const HOSTNAME_LABEL = 'Hostname in messages';

export const GENERATE_SECRET_LABEL = 'Generate';

export const SECRET_SAVED_PLACEHOLDER = 'Saved — leave blank to keep';

export const SECRET_CLEAR_LABEL = 'Remove';

export const SIGNATURE_HINT =
  'Each request carries a Zone-Signature header, an HMAC-SHA256 of the timestamp and body under this secret.';

export const UNENCRYPTED_HTTP_WARNING =
  'This URL uses plain http, so audit events travel unencrypted. Use https unless the receiver is on a network you trust.';

export const UNENCRYPTED_ENDPOINT_WARNING =
  'This endpoint uses plain http, so audit events and request signatures travel unencrypted. Use https unless the store is on a network you trust.';

export const UNENCRYPTED_UDP_WARNING =
  'Syslog over UDP is unencrypted and best effort: messages can be read in transit or lost without a retry. Use TLS unless the receiver is on a network you trust.';

export const BACKLOG_LABEL = 'Backlog';

export const UP_TO_DATE = 'Up to date';

export const LAST_SUCCESS_LABEL = 'Last delivered';

export const NEVER_DELIVERED = 'Nothing delivered yet';

export const LAST_ERROR_LABEL = 'Last error';

export const NEXT_ATTEMPT_LABEL = 'Next attempt';

export const DELIVERED_COUNT_LABEL = 'Delivered';

export const TEST_DELIVERED = 'The test event was delivered.';

export const TEST_NOT_DELIVERED = "The test event wasn't delivered. {}";

export const STALLED_NOTICE =
  'Delivery is waiting behind a database transaction that has been open since {}. Events written after it are sent once it finishes; nothing is lost.';

export const EVENT_SINGULAR = 'event';

export const EVENT_PLURAL = 'events';

export const DAY_SINGULAR = 'day';

export const DAY_PLURAL = 'days';

export const BACKLOG_WAITING = '{} waiting';

export const DELETE_CONFIRMATION =
  "Delete {}? Zone stops sending audit events to it and forgets its secrets. Events it hasn't received are no longer held back from retention.";

export const RETENTION_TITLE = 'Retention';

export const RETENTION_DESCRIPTION =
  'Delete audit events older than a number of days. An event is deleted only after every enabled destination has delivered it.';

export const RETENTION_DAYS_LABEL = 'Keep events for (days)';

export const KEEP_FOREVER_LABEL = 'Keep forever';

export const SAVE_RETENTION_LABEL = 'Save retention';

export const KEPT_FOREVER = 'Audit events are kept forever.';

export const RETENTION_PERIOD =
  'Events older than {} are deleted once every enabled destination has delivered them.';

export const RETENTION_PERIOD_SAVED =
  'A period of {} is saved and applies again once a Zone license is installed. Until then, audit events are kept forever.';

export const RETENTION_NOT_APPLIED = 'Retention applies only while a Zone license is installed.';

export const EXPIRED_COUNT = '{} past the retention period';

export const HELD_COUNT = '{} kept until delivered';

export const DELETED_COUNT = '{} deleted';

export const CUTOFF_LABEL = 'Deleting events before';

export const EXPIRED_LABEL = 'Past the retention period';

export const HELD_LABEL = 'Waiting for delivery';

export const HOLDERS_LABEL = 'Waiting on';

export const LAST_SWEEP_LABEL = 'Last sweep';

export const NOT_SWEPT = 'Not swept yet';

export const DOWNLOAD_TITLE = 'Download';

export const DOWNLOAD_DESCRIPTION =
  'Download the audit log for a date range as NDJSON, one event per line, or as CSV. Dates are in UTC and include the whole end day.';

export const DOWNLOAD_INTERRUPTED = 'The download stopped part way. Try again.';

export const DOWNLOAD_MEMORY_NOTE =
  'Your browser holds the whole file in memory until the download finishes. For very large ranges, use the API with curl.';

export const START_DATE_LABEL = 'From';

export const END_DATE_LABEL = 'To';

export const FORMAT_LABEL = 'Format';

export const DOWNLOAD_LABEL = 'Download';

export const LOADING_LABEL = 'Loading audit export...';

export const RETRY_LABEL = 'Retry';

export const LICENSE_LINK_LABEL = 'Go to License';

export const RENEW_LINK_LABEL = 'Renew on the License page';

export const NO_DESTINATIONS_READ_ONLY = 'No destinations are set up.';

export const UNENCRYPTED_LABEL = 'Unencrypted';

export const ENABLED_DESCRIPTION = 'Send audit events to this destination.';

export const NAME_PLACEHOLDER = 'Splunk';

export const URL_PLACEHOLDER = 'https://siem.example.com/zone/audit';

export const BUCKET_PLACEHOLDER = 'acme-audit';

export const REGION_PLACEHOLDER = 'us-east-1';

export const ENDPOINT_PLACEHOLDER = 'https://minio.internal:9000';

export const PREFIX_PLACEHOLDER = 'zone/audit/';

export const HOST_PLACEHOLDER = 'siem.internal';

export const CERTIFICATES_REMOVED = 'Saving removes the saved certificates.';

export const CERTIFICATES_HINT =
  'Optional PEM text. Add a CA certificate to trust a private CA, and a client certificate and key together for mutual TLS.';

export const SECRET_REMOVED = 'Removed when you save.';

export const SECRET_KEEP_LABEL = 'Keep';

export const NAME_REQUIRED = 'Enter a name for this destination.';

export const URL_REQUIRED = 'Enter the URL that receives audit events.';

export const BUCKET_REQUIRED = 'Enter the bucket name.';

export const REGION_REQUIRED = 'Enter the region, like us-east-1.';

export const HOST_REQUIRED = 'Enter the syslog host.';

export const SECRET_REQUIRED = 'Enter this secret.';

export const CLIENT_PAIR_REQUIRED = 'Enter the client certificate and key together.';

export const RETENTION_SAVED = 'Retention saved.';

export const RETENTION_DAYS_HINT = 'At least 1 day.';

export const DOWNLOADED_LABEL = 'Downloaded';

export const S3_SCHEME = 's3://';
