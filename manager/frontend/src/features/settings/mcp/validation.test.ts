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

  it('names both kinds of address it refuses', () => {
    expect(URL_METADATA).toBe('Do not use a link-local or cloud metadata address');
  });

  it.each([
    ['http://169.254.0.1/'],
    ['http://169.254.169.253/mcp'],
    ['https://169.254.255.255:8443/mcp'],
    ['http://100.100.100.200/'],
    ['http://168.63.129.16/machine'],
    ['http://[fe80::1]/'],
    ['http://[FE80::1]:8080/mcp'],
    ['http://[febf:ffff::1]/'],
    ['http://[fd00:ec2::254]/'],
    ['http://[fd20:ce::254]/'],
    ['http://[64:ff9b:1::a9fe:a9fe]/'],
    ['http://[64:ff9b:1:ffff::1]/'],
  ])('refuses %p, a link-local or metadata address', (url) => {
    expect(urlError(url)).toBe(URL_METADATA);
  });

  it.each([
    ['http://[::ffff:169.254.169.254]/'],
    ['http://[::ffff:a9fe:a9fe]/'],
    ['http://[::169.254.169.254]/'],
    ['http://[64:ff9b::a9fe:a9fe]/'],
    ['http://[64:ff9b::6464:64c8]/'],
    ['http://[::ffff:168.63.129.16]/'],
  ])('refuses %p, an IPv6 address that carries a refused IPv4 one', (url) => {
    expect(urlError(url)).toBe(URL_METADATA);
  });

  it.each([
    ['http://2852039166/'],
    ['http://0xa9.0xfe.0xa9.0xfe/'],
    ['http://0251.0376.0251.0376/'],
    ['http://169.254.43518/'],
    ['http://169.254.169.254./'],
    ['http://169.254.169.254../'],
  ])('refuses %p, another spelling of the metadata address', (url) => {
    expect(urlError(url)).toBe(URL_METADATA);
  });

  it.each([
    ['http://metadata.google.internal/computeMetadata/v1/'],
    ['http://METADATA.GOOGLE.INTERNAL/'],
    ['http://metadata.google.internal./'],
    ['http://%6d%65%74%61%64%61%74%61.google.internal/'],
    ['https://metadata.goog/'],
  ])('refuses %p, a metadata host name', (url) => {
    expect(urlError(url)).toBe(URL_METADATA);
  });

  it.each([
    ['http://127.0.0.1:8080/mcp'],
    ['https://h/mcp?x=1'],
    ['http://192.168.1.10/mcp'],
    ['http://169.253.255.255/mcp'],
    ['http://169.255.0.1/mcp'],
    ['http://100.100.100.100/'],
    ['http://168.63.129.17/'],
    ['http://168.63.128.16/'],
    ['http://10.0.0.5/'],
    ['http://172.17.0.1/'],
    ['http://[::1]:8080/mcp'],
    ['http://[fd00::1]/'],
    ['http://[fd00:ec2::253]/'],
    ['http://[fd20:ce::253]/'],
    ['http://[fd20::254]/'],
    ['http://[fe7f::1]/'],
    ['http://[fec0::1]/'],
    ['http://[64:ff9b:2::a9fe:a9fe]/'],
    ['http://[64:ff9b::808:808]/'],
    ['http://[::ffff:192.168.1.20]/'],
    ['http://[2606:2800:220:1:248:1893:25c8:1946]/'],
    ['http://localhost/mcp'],
    ['http://metadata/'],
    ['http://metadata.google.internal.example/'],
    ['http://gateway.internal/'],
    ['http://ollama.lan/'],
  ])('accepts %p, which is not or only resembles a refused address', (url) => {
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

  it.each([
    ['Metadata-Flavor'],
    ['METADATA'],
    ['X-aws-ec2-metadata-token'],
    ['X-Aws-Ec2-Metadata-Token-Ttl-Seconds'],
    ['X-Google-Metadata-Request'],
    ['x-aliyun-ecs-metadata-token'],
    ['X-Aliyun-Ecs-Metadata-Token-TTL-Seconds'],
  ])('refuses %p, which a cloud metadata service requires', (name) => {
    expect(headerNameError(name)).toBe(HEADER_RESERVED);
  });

  it.each([
    ['Authorization'],
    ['X-Content-Type'],
    ['Accept-Language'],
    ['Mcp-Parameter'],
    ['X-Metadata-Note'],
  ])('accepts %p, which only resembles a reserved name', (name) => {
    expect(headerNameError(name)).toBeNull();
  });
});
