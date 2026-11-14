import { describe, expect, it } from 'bun:test';
import { isWebUrl, NOT_WEB_URL_MESSAGE, requireWebUrl } from './webUrl';

const WEB_URLS = [
  'https://github.com/apps/zone-acme',
  'http://github.example.test/settings/apps/new?state=signed',
  'HTTPS://GITHUB.COM/acme',
];

const REFUSED_URLS = [
  'javascript:alert(1)',
  'JavaScript:alert(1)',
  ' javascript:alert(1)',
  'java\tscript:alert(1)',
  'data:text/html,<script>alert(1)</script>',
  'vbscript:msgbox(1)',
  'file:///etc/passwd',
  'mailto:security@github.com',
  '/settings/apps/new',
  '//github.com/apps/zone-acme',
  'github.com/apps/zone-acme',
  '',
];

describe('isWebUrl', () => {
  it.each(WEB_URLS)('accepts the absolute web URL %p', (url) => {
    expect(isWebUrl(url)).toBe(true);
  });

  it.each(REFUSED_URLS)('refuses %p', (url) => {
    expect(isWebUrl(url)).toBe(false);
  });
});

describe('requireWebUrl', () => {
  it('returns a web URL unchanged', () => {
    expect(requireWebUrl(WEB_URLS[0])).toBe(WEB_URLS[0]);
  });

  it('throws without echoing the refused address', () => {
    expect(() => requireWebUrl('javascript:alert(1)')).toThrow(NOT_WEB_URL_MESSAGE);
  });
});
