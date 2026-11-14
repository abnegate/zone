import { POLL_INTERVAL, Status } from '../constants';
import type { McpServer } from '../types';

export function pollInterval(servers: readonly McpServer[] | undefined): number | false {
  return servers?.some((server) => server.status === Status.Connecting) ? POLL_INTERVAL : false;
}
