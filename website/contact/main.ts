import type { Context } from './Context.ts';
import { parseForm } from './form.ts';
import { compose, send } from './mail.ts';
import { OutcomeKind } from './Outcome.ts';
import { type Settings, settingsFrom } from './Settings.ts';

const html = { 'content-type': 'text/html; charset=utf-8' } as const;

function page(heading: string, body: string): string {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${heading}</title></head><body><h1>${heading}</h1><p>${body}</p><p>Use your browser&rsquo;s back button to return to your message.</p></body></html>`;
}

export async function handle(
  { req, res, error }: Context,
  settings: Settings,
  fetcher: typeof fetch = fetch
): Promise<unknown> {
  if (req.method !== 'POST') {
    return res.text('Only POST is accepted.', 405, { allow: 'POST' });
  }

  const outcome = parseForm(req.bodyText, settings.origins);
  switch (outcome.kind) {
    case OutcomeKind.Invalid:
      return res.text(page('Your message was not sent', outcome.problem), 400, html);
    case OutcomeKind.Trapped:
      return res.text('', 303, { location: outcome.redirect.href });
    case OutcomeKind.Accepted:
      try {
        await send(compose(outcome.submission, settings), settings.resendKey, fetcher);
      } catch (failure) {
        error(String(failure));
        return res.text(
          page(
            'Your message was not sent',
            `Something went wrong on our side. Please try again, or write to ${settings.recipient}.`
          ),
          502,
          html
        );
      }
      return res.text('', 303, { location: outcome.submission.redirect.href });
  }
}

export default async (context: Context): Promise<unknown> =>
  handle(context, settingsFrom(process.env));
