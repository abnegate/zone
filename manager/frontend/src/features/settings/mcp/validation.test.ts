import { describe, expect, it } from 'bun:test';
import {
  ENVIRONMENT_NAME,
  environmentNameError,
  HEADER_NAME,
  HEADER_RESERVED,
  headerNameError,
  LITERAL,
  literalError,
  NAME_CHARACTERS,
  NAME_LENGTH,
  NAME_REQUIRED,
  NAME_SEPARATOR,
  NAME_TRAILING,
  nameError,
  URL_CREDENTIALS,
  URL_FRAGMENT,
  URL_INVALID,
  URL_METADATA,
  URL_PROTOCOL,
  URL_REQUIRED,
  urlError,
} from './validation';

describe('nameError', () => {
  it.each([
    ['a__b', NAME_SEPARATOR],
    ['a_', NAME_TRAILING],
    ['a.b', NAME_CHARACTERS],
    ['a b', NAME_CHARACTERS],
    ['a'.repeat(33), NAME_LENGTH],
    ['', NAME_REQUIRED],
  ])('refuses %p', (name, message) => {
    expect(nameError(name)).toBe(message);
  });

  it('names the 32-character limit', () => {
    expect(NAME_LENGTH).toBe('Use at most 32 characters');
  });

  it.each([['a'.repeat(32)], ['my-docs_1']])('accepts %p', (name) => {
    expect(nameError(name)).toBeNull();
  });
});

describe('urlError', () => {
  it.each([
    ['ftp://x', URL_PROTOCOL],
    ['https://u:p@h', URL_CREDENTIALS],
    ['https://h/#f', URL_FRAGMENT],
    ['https://h/#', URL_FRAGMENT],
    ['not a url', URL_INVALID],
    ['http://169.254.169.254/latest/meta-data/', URL_METADATA],
    ['https://169.254.169.254:8443/mcp', URL_METADATA],
    ['http://0xa9fea9fe/', URL_METADATA],
    ['', URL_REQUIRED],
  ])('refuses %p', (url, message) => {
    expect(urlError(url)).toBe(message);
  });

  it.each([
    ['http://127.0.0.1:8080/mcp'],
    ['https://h/mcp?x=1'],
    ['http://169.254.169.253/mcp'],
    ['http://192.168.1.10/mcp'],
  ])('accepts %p', (url) => {
    expect(urlError(url)).toBeNull();
  });
});

describe('literalError', () => {
  it('refuses an environment reference', () => {
    expect(literalError(`\${HOME}`)).toBe(LITERAL);
    expect(LITERAL).toBe(`Replace \${…} references with the value`);
  });

  it('accepts a plain value and a lone dollar sign', () => {
    expect(literalError('Bearer $token')).toBeNull();
  });
});

describe('environmentNameError', () => {
  it('accepts a variable name and refuses one starting with a digit', () => {
    expect(environmentNameError('_API_KEY2')).toBeNull();
    expect(environmentNameError('2KEY')).toBe(ENVIRONMENT_NAME);
    expect(environmentNameError('API-KEY')).toBe(ENVIRONMENT_NAME);
  });
});

describe('headerNameError', () => {
  it('accepts a token and refuses separators', () => {
    expect(headerNameError('X-Api-Key')).toBeNull();
    expect(headerNameError('X Api')).toBe(HEADER_NAME);
    expect(headerNameError('X:Api')).toBe(HEADER_NAME);
    expect(headerNameError('')).toBe(HEADER_NAME);
  });

  it.each([
    ['Content-Type'],
    ['accept'],
    ['HOST'],
    ['Mcp-Session-Id'],
    ['Mcp-Param-Region'],
    ['mcp-param-'],
  ])('refuses %p, which the connection sets itself', (name) => {
    expect(headerNameError(name)).toBe(HEADER_RESERVED);
  });

  it.each([['Authorization'], ['X-Content-Type'], ['Accept-Language'], ['Mcp-Parameter']])(
    'accepts %p, which only resembles a reserved name',
    (name) => {
      expect(headerNameError(name)).toBeNull();
    }
  );
});
