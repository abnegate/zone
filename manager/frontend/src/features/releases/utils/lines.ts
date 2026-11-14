import type { SourceOptions, TagLine } from '../types';
import { parseTag } from './tags';

export const UNPREFIXED = 'No prefix';

export function tagLines(source: SourceOptions): readonly TagLine[] {
  if (source.lines.length > 0) return source.lines;
  return [
    {
      prefix: null,
      latest_tag: source.latest_tag,
      next_tag: source.next_tag,
      previous_tags: source.previous_tags,
    },
  ];
}

export function dominantLine(source: SourceOptions): TagLine {
  const lines = tagLines(source);
  return lines.find((line) => line.next_tag === source.next_tag) ?? lines[0];
}

export function lineLabel(line: TagLine): string {
  return line.prefix ?? UNPREFIXED;
}

export function lineOfTag(source: SourceOptions, tag: string): TagLine | null {
  const parsed = parseTag(tag);
  if (!parsed) return null;
  return tagLines(source).find((line) => line.prefix === parsed.prefix) ?? null;
}

export function expectedLatestTag(source: SourceOptions, tag: string): string | null {
  return lineOfTag(source, tag)?.latest_tag ?? null;
}
