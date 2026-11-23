import { describe, expect, test } from 'bun:test';
import type { Context } from './Context.ts';
import { allowedRedirect, parseForm } from './form.ts';
import { compose, resendURL, send } from './mail.ts';
import { handle } from './main.ts';
import { OutcomeKind } from './Outcome.ts';
import { type Settings, settingsFrom } from './Settings.ts';

const settings: Settings = {
  resendKey: 're_test',
  recipient: 'zone@bitecode.ltd',
  sender: 'Zone website <contact@bitecode.ltd>',
  origins: ['https://zone.bitecode.ltd', 'https://bitecodehq.github.io'],
};

const sent = 'https://zone.bitecode.ltd/contact/sent/';

function body(fields: Readonly<Record<string, string>>): string {
  return new URLSearchParams({
    name: 'Ada Lovelace',
    email: 'ada@example.com',
    company: 'Analytical Engines',
    message: 'We would like Enterprise for 40 seats.',
    website: '',
    redirect: sent,
    ...fields,
  }).toString();
}

interface Reply {
  readonly body: string;
  readonly status: number;
  readonly headers: Readonly<Record<string, string>>;
}

function context(method: string, bodyText: string, errors: string[] = []): Context {
  return {
    req: { method, bodyText },
    res: {
      text: (
        text: string,
        status = 200,
        headers: Readonly<Record<string, string>> = {}
      ): Reply => ({
        body: text,
        status,
        headers,
      }),
    },
    error: (message: string) => {
      errors.push(message);
    },
  };
}

interface Call {
  readonly url: string;
  readonly headers: Headers;
  readonly body: string;
}

function recordingFetch(status = 200): { fetcher: typeof fetch; calls: Call[] } {
  const calls: Call[] = [];
  const fetcher = (async (input: string | URL | Request, init?: RequestInit) => {
    calls.push({
      url: input instanceof Request ? input.url : input.toString(),
      headers: new Headers(init?.headers),
      body: String(init?.body ?? ''),
    });
    return new Response(status === 200 ? '{"id":"1"}' : '{"message":"nope"}', { status });
  }) as typeof fetch;
  return { fetcher, calls };
}

describe('settings', () => {
  test('reads every variable and normalises origins', () => {
    const read = settingsFrom({
      RESEND_API_KEY: 're_x',
      CONTACT_RECIPIENT: 'zone@bitecode.ltd',
      CONTACT_SENDER: 'Zone <contact@bitecode.ltd>',
      CONTACT_ORIGINS: 'https://zone.bitecode.ltd/, https://bitecodehq.github.io/zone/',
    });
    expect(read.origins).toEqual(['https://zone.bitecode.ltd', 'https://bitecodehq.github.io']);
  });

  test('refuses to start without the Resend key', () => {
    expect(() =>
      settingsFrom({
        CONTACT_RECIPIENT: 'a@b.c',
        CONTACT_SENDER: 'a@b.c',
        CONTACT_ORIGINS: 'https://a.b',
      })
    ).toThrow('RESEND_API_KEY');
  });
});

describe('form', () => {
  test('accepts a complete submission', () => {
    const outcome = parseForm(body({}), settings.origins);
    expect(outcome.kind).toBe(OutcomeKind.Accepted);
  });

  test('accepts a submission without a company', () => {
    expect(parseForm(body({ company: '' }), settings.origins).kind).toBe(OutcomeKind.Accepted);
  });

  for (const field of ['name', 'email', 'message']) {
    test(`rejects a missing ${field}`, () => {
      expect(parseForm(body({ [field]: '  ' }), settings.origins).kind).toBe(OutcomeKind.Invalid);
    });
  }

  test('rejects an address that is not an email', () => {
    expect(parseForm(body({ email: 'ada' }), settings.origins).kind).toBe(OutcomeKind.Invalid);
  });

  test('rejects a message over the form limit', () => {
    const outcome = parseForm(body({ message: 'x'.repeat(5001) }), settings.origins);
    expect(outcome.kind).toBe(OutcomeKind.Invalid);
  });

  test('traps a filled honeypot without reporting it', () => {
    const outcome = parseForm(body({ website: 'https://spam.example' }), settings.origins);
    expect(outcome).toEqual({ kind: OutcomeKind.Trapped, redirect: new URL(sent) });
  });

  test('only redirects to the site itself', () => {
    expect(allowedRedirect(sent, settings.origins)?.href).toBe(sent);
    expect(
      allowedRedirect('https://bitecodehq.github.io/zone/contact/sent/', settings.origins)
    ).toBeDefined();
    expect(allowedRedirect('https://evil.example/contact/sent/', settings.origins)).toBeUndefined();
    expect(
      allowedRedirect('https://zone.bitecode.ltd.evil.example/', settings.origins)
    ).toBeUndefined();
    expect(allowedRedirect('javascript:alert(1)', settings.origins)).toBeUndefined();
    expect(allowedRedirect('', settings.origins)).toBeUndefined();
  });

  test('rejects a form posted from another site', () => {
    const outcome = parseForm(body({ redirect: 'https://evil.example/' }), settings.origins);
    expect(outcome.kind).toBe(OutcomeKind.Invalid);
  });
});

describe('mail', () => {
  const submission = {
    name: 'Ada\r\nBcc: victim@example.com',
    email: 'ada@example.com',
    company: '',
    message: 'Hello',
    redirect: new URL(sent),
  };

  test('replies to the visitor and keeps the subject on one line', () => {
    const email = compose(submission, settings);
    expect(email.to).toEqual(['zone@bitecode.ltd']);
    expect(email.from).toBe(settings.sender);
    expect(email.reply_to).toBe('ada@example.com');
    expect(email.subject).not.toMatch(/[\r\n]/);
    expect(email.text).toContain('Company: -');
  });

  test('posts the email to Resend with the key', async () => {
    const { fetcher, calls } = recordingFetch();
    await send(compose(submission, settings), 're_key', fetcher);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.url).toBe(resendURL);
    expect(calls[0]?.headers.get('authorization')).toBe('Bearer re_key');
    expect(JSON.parse(calls[0]?.body ?? '{}')).toMatchObject({ reply_to: 'ada@example.com' });
  });

  test('fails when Resend refuses', async () => {
    const { fetcher } = recordingFetch(422);
    expect(send(compose(submission, settings), 're_key', fetcher)).rejects.toThrow('422');
  });
});

describe('handler', () => {
  test('refuses anything but POST', async () => {
    const reply = (await handle(context('GET', ''), settings)) as Reply;
    expect(reply.status).toBe(405);
  });

  test('sends the message and returns to the thank-you page', async () => {
    const { fetcher, calls } = recordingFetch();
    const reply = (await handle(context('POST', body({})), settings, fetcher)) as Reply;
    expect(calls).toHaveLength(1);
    expect(reply.status).toBe(303);
    expect(reply.headers.location).toBe(sent);
  });

  test('sends nothing for a trapped bot but looks the same', async () => {
    const { fetcher, calls } = recordingFetch();
    const reply = (await handle(
      context('POST', body({ website: 'x' })),
      settings,
      fetcher
    )) as Reply;
    expect(calls).toHaveLength(0);
    expect(reply.status).toBe(303);
    expect(reply.headers.location).toBe(sent);
  });

  test('explains an invalid form without sending', async () => {
    const { fetcher, calls } = recordingFetch();
    const reply = (await handle(context('POST', body({ email: 'x' })), settings, fetcher)) as Reply;
    expect(calls).toHaveLength(0);
    expect(reply.status).toBe(400);
    expect(reply.body).toContain('email address');
  });

  test('logs a Resend failure and says so', async () => {
    const { fetcher } = recordingFetch(500);
    const errors: string[] = [];
    const reply = (await handle(context('POST', body({}), errors), settings, fetcher)) as Reply;
    expect(reply.status).toBe(502);
    expect(reply.body).toContain(settings.recipient);
    expect(errors[0]).toContain('500');
  });
});
