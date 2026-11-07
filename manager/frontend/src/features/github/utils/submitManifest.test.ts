import { afterEach, beforeEach, describe, expect, it, type Mock, spyOn } from 'bun:test';
import { submitManifest } from './submitManifest';
import { NOT_WEB_URL_MESSAGE } from './webUrl';

const ACTION = 'https://github.com/settings/apps/new?state=signed-state';

describe('submitManifest', () => {
  let submit: Mock<() => void>;
  let submitted: HTMLFormElement[];

  beforeEach(() => {
    submitted = [];
    submit = spyOn(HTMLFormElement.prototype, 'submit').mockImplementation(function (
      this: HTMLFormElement
    ) {
      submitted.push(this);
    });
  });

  afterEach(() => {
    submit.mockRestore();
    for (const form of document.body.querySelectorAll('form')) {
      form.remove();
    }
  });

  function submitOnce(manifest: string): HTMLFormElement {
    submitManifest({ action: ACTION, manifest });
    expect(submit).toHaveBeenCalledTimes(1);
    expect(submitted).toHaveLength(1);
    return submitted[0];
  }

  it('posts a hidden form to the registration action from the document body', () => {
    const form = submitOnce('{"name":"Zone"}');

    expect(form.method).toBe('post');
    expect(form.getAttribute('method')).toBe('post');
    expect(form.action).toBe(ACTION);
    expect(form.hidden).toBe(true);
    expect(form.parentElement).toBe(document.body);
  });

  it('carries only the manifest field, since the state travels in the action', () => {
    const form = submitOnce('{"name":"Zone"}');

    const inputs = form.querySelectorAll('input');
    expect(inputs).toHaveLength(1);
    expect(form.elements).toHaveLength(1);
    expect(inputs[0].name).toBe('manifest');
    expect(inputs[0].type).toBe('hidden');
  });

  it('keeps the manifest verbatim, including quotes, ampersands, markup and non-ASCII text', () => {
    const manifest = JSON.stringify({
      name: 'Zone "dev" & <team>',
      description: "It's naïve — 日本語 ✓",
      url: 'https://zone.test/?a=1&b=2',
    });

    const form = submitOnce(manifest);

    expect(form.querySelector('input')?.value).toBe(manifest);
    expect(new FormData(form).get('manifest')).toBe(manifest);
  });

  it('builds the form in the given document', () => {
    const target = document.implementation.createHTMLDocument('target');

    submitManifest({ action: ACTION, manifest: '{}' }, target);

    expect(submitted).toHaveLength(1);
    expect(submitted[0].ownerDocument).toBe(target);
    expect(submitted[0].parentElement).toBe(target.body);
    expect(document.body.querySelector('form')).toBeNull();
  });

  it('refuses a javascript: action without building or posting the form', () => {
    expect(() =>
      submitManifest({ action: 'javascript:alert(1)', manifest: '{"name":"Zone"}' })
    ).toThrow(NOT_WEB_URL_MESSAGE);

    expect(submit).not.toHaveBeenCalled();
    expect(document.body.querySelector('form')).toBeNull();
  });
});
