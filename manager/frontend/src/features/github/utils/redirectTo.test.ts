import { afterEach, beforeEach, describe, expect, it, type Mock, spyOn } from 'bun:test';
import { redirectTo } from './redirectTo';
import { NOT_WEB_URL_MESSAGE } from './webUrl';

describe('redirectTo', () => {
  let assign: Mock<(url: string | URL) => void>;

  beforeEach(() => {
    assign = spyOn(window.location, 'assign').mockImplementation(() => {});
  });

  afterEach(() => {
    assign.mockRestore();
  });

  it('navigates to an https URL', () => {
    const url = 'https://github.com/apps/zone-acme/installations/new?state=state-2';

    redirectTo(url);

    expect(assign).toHaveBeenCalledTimes(1);
    expect(assign).toHaveBeenCalledWith(url);
  });

  it('refuses a javascript: URL without navigating', () => {
    expect(() => redirectTo('javascript:alert(1)')).toThrow(NOT_WEB_URL_MESSAGE);
    expect(assign).not.toHaveBeenCalled();
  });

  it('refuses a relative URL without navigating', () => {
    expect(() => redirectTo('/apps/zone-acme')).toThrow(NOT_WEB_URL_MESSAGE);
    expect(assign).not.toHaveBeenCalled();
  });
});
