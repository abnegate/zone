const LINE_BREAK = /\s*⏎\s*|\r?\n/;
const HEADING = /^#{1,6}\s+/;
const QUOTE = /^>\s?/;
const LIST_MARKER = /^(?:[-*+]|\d+[.)])\s+/;
const FENCE = /^```/;
const RULE = /^(?:-{3,}|\*{3,}|_{3,})$/;
const IMAGE = /!\[([^\]]*)\]\([^)]*\)/g;
const LINK = /\[([^\]]+)\]\([^)]*\)/g;
const STRONG = /(\*\*|__)(.+?)\1/g;
const EMPHASIS = /(^|[^\w*_])[*_]([^*_\s][^*_]*?)[*_](?![\w*_])/g;
const CODE = /`+([^`]*)`+/g;
const STRIKE = /~~(.+?)~~/g;
const DANGLING_STRONG = /\*\*/g;
const WHITESPACE = /\s+/g;

/// A search snippet is a slice of markdown written for a renderer, shown where
/// nothing renders it: the marks come off and the lines the server collapsed
/// onto one — with a return glyph standing in for each break — read as prose.
export function toPlainText(markdown: string): string {
  return markdown
    .split(LINE_BREAK)
    .map((line) => line.trim().replace(HEADING, '').replace(QUOTE, '').replace(LIST_MARKER, ''))
    .filter((line) => line && !FENCE.test(line) && !RULE.test(line))
    .join(' ')
    .replace(IMAGE, '$1')
    .replace(LINK, '$1')
    .replace(STRONG, '$2')
    .replace(EMPHASIS, '$1$2')
    .replace(CODE, '$1')
    .replace(STRIKE, '$1')
    .replace(DANGLING_STRONG, '')
    .replace(WHITESPACE, ' ')
    .trim();
}
