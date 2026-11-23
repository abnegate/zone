import { type Outcome, OutcomeKind } from './Outcome.ts';

const limits = {
  name: 200,
  email: 320,
  company: 200,
  message: 5000,
} as const;

const emailPattern = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

function invalid(problem: string): Outcome {
  return { kind: OutcomeKind.Invalid, problem };
}

export function allowedRedirect(value: string, origins: ReadonlyArray<string>): URL | undefined {
  try {
    const url = new URL(value);
    return origins.includes(url.origin) ? url : undefined;
  } catch {
    return undefined;
  }
}

export function parseForm(body: string, origins: ReadonlyArray<string>): Outcome {
  const form = new URLSearchParams(body);
  const field = (name: string): string => form.get(name)?.trim() ?? '';

  const redirect = allowedRedirect(field('redirect'), origins);
  if (redirect === undefined) {
    return invalid('The form was sent from a page this endpoint does not serve.');
  }
  if (field('website') !== '') {
    return { kind: OutcomeKind.Trapped, redirect };
  }

  const name = field('name');
  const email = field('email');
  const company = field('company');
  const message = field('message');

  if (name === '' || email === '' || message === '') {
    return invalid('Name, email and message are required.');
  }
  if (!emailPattern.test(email)) {
    return invalid('That email address does not look right.');
  }
  for (const [key, limit] of Object.entries(limits)) {
    if (field(key).length > limit) {
      return invalid(`The ${key} is longer than ${limit} characters.`);
    }
  }

  return {
    kind: OutcomeKind.Accepted,
    submission: { name, email, company, message, redirect },
  };
}
