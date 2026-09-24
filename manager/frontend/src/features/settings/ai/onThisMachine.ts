const LOOPBACK_NAMES = ['localhost', '127.0.0.1', '[::1]'];
const LOOPBACK_SUFFIX = '.localhost';

/** Whether a console at `hostname` is open in a browser on the machine Zone runs on. */
export function onThisMachine(hostname: string): boolean {
  const name = hostname.toLowerCase();
  return (
    LOOPBACK_NAMES.includes(name) ||
    (name.endsWith(LOOPBACK_SUFFIX) && name.split('.').every((label) => label.length > 0))
  );
}
