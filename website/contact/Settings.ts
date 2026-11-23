export interface Settings {
  readonly resendKey: string;
  readonly recipient: string;
  readonly sender: string;
  readonly origins: ReadonlyArray<string>;
}

export function settingsFrom(environment: Readonly<Record<string, string | undefined>>): Settings {
  const required = (name: string): string => {
    const value = environment[name]?.trim();
    if (!value) {
      throw new Error(`${name} is not set`);
    }
    return value;
  };
  return {
    resendKey: required('RESEND_API_KEY'),
    recipient: required('CONTACT_RECIPIENT'),
    sender: required('CONTACT_SENDER'),
    origins: required('CONTACT_ORIGINS')
      .split(',')
      .map((origin) => new URL(origin.trim()).origin),
  };
}
