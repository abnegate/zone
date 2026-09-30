import { readFileSync } from 'node:fs';

function literal(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function pattern(selector: string): string {
  return selector
    .trim()
    .split(/\s*,\s*/)
    .map((part) => part.split(/\s+/).map(literal).join('\\s+'))
    .join('\\s*,\\s*');
}

function collapse(text: string): string {
  return text.replace(/\s+/g, ' ').trim();
}

export function read(path: string): string {
  return readFileSync(path, 'utf8');
}

export function token(css: string, name: string): string {
  const match = css.match(new RegExp(`--${name}:\\s*([^;]+);`));
  if (!match) throw new Error(`token --${name} is not defined`);
  return match[1].trim();
}

export function rule(css: string, selector: string): string {
  const start = '(?:^|[{};]|\\*/)\\s*';
  const match = css.match(new RegExp(`${start}${pattern(selector)}\\s*\\{([^}]*)\\}`));
  if (!match) throw new Error(`rule ${selector} is not defined`);
  return collapse(match[1]);
}

export function media(css: string, query: string): string {
  const condition = literal(query.trim())
    .replace(/\s*(\\\(|\\\)|:|,)\s*/g, '\\s*$1\\s*')
    .replace(/\s+/g, '\\s+');
  const header = css.match(new RegExp(`@media\\s+${condition}\\s*\\{`));
  if (header?.index === undefined) throw new Error(`@media ${query} is not defined`);
  const open = header.index + header[0].length - 1;
  let depth = 0;
  for (let index = open; index < css.length; index++) {
    if (css[index] === '{') depth++;
    if (css[index] === '}' && --depth === 0) return css.slice(open + 1, index);
  }
  throw new Error(`@media ${query} is not closed`);
}
