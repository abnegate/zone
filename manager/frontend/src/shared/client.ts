/**
 * The console inside Zone's desktop and mobile client.
 *
 * The client is a webview onto the same console the browser gets, so the page
 * cannot assume the host is there: everything here reports its absence rather
 * than throwing, and the caller is expected to have a path that works in a
 * plain browser too.
 */

interface ClientHost {
  invoke(command: string, payload?: Record<string, unknown>): Promise<unknown>;
}

function host(): ClientHost | null {
  const internals = (globalThis as { __TAURI_INTERNALS__?: ClientHost }).__TAURI_INTERNALS__;
  return typeof internals?.invoke === 'function' ? internals : null;
}

/** Whether the page is running inside the desktop or mobile client. */
export function inClient(): boolean {
  return host() !== null;
}

/**
 * What the host said, or that it did not answer.
 *
 * Kept apart because plenty of what the host is asked answers with nothing at
 * all when it worked: a reply of `null` is not a failure, and a caller that
 * cannot tell the two apart reports a notification as unshown every time one is
 * shown.
 */
export type ClientAnswer<T> = { answered: true; value: T } | { answered: false };

/**
 * Asks the host to do something, reporting what it answered.
 *
 * Nothing throws: in a browser there is no host, and a host that refuses logs
 * the detail. Neither is worth breaking a render over, so both come back as an
 * unanswered call for the caller to decide about.
 */
export async function askClient<T>(
  command: string,
  payload?: Record<string, unknown>
): Promise<ClientAnswer<T>> {
  const client = host();
  if (!client) return { answered: false };
  try {
    return { answered: true, value: (await client.invoke(command, payload)) as T };
  } catch (error) {
    console.warn(`Zone client refused ${command}`, error);
    return { answered: false };
  }
}
