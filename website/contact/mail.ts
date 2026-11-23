import type { Settings } from './Settings.ts';
import type { Submission } from './Submission.ts';

export const resendURL = 'https://api.resend.com/emails';

export interface Email {
  readonly from: string;
  readonly to: ReadonlyArray<string>;
  readonly reply_to: string;
  readonly subject: string;
  readonly text: string;
}

function singleLine(value: string): string {
  return value.replace(/[\r\n]+/g, ' ');
}

export function compose(submission: Submission, settings: Settings): Email {
  const { name, email, company, message } = submission;
  const sender = company === '' ? name : `${name}, ${company}`;
  return {
    from: settings.sender,
    to: [settings.recipient],
    reply_to: email,
    subject: singleLine(`Website enquiry from ${sender}`),
    text: [
      `Name: ${name}`,
      `Email: ${email}`,
      `Company: ${company === '' ? '-' : company}`,
      '',
      message,
    ].join('\n'),
  };
}

export async function send(
  email: Email,
  key: string,
  fetcher: typeof fetch = fetch
): Promise<void> {
  const response = await fetcher(resendURL, {
    method: 'POST',
    headers: {
      authorization: `Bearer ${key}`,
      'content-type': 'application/json',
    },
    body: JSON.stringify(email),
  });
  if (!response.ok) {
    throw new Error(`Resend answered ${response.status}: ${await response.text()}`);
  }
}
