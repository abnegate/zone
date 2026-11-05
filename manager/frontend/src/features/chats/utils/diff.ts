export type DiffLineKind = 'added' | 'removed' | 'context' | 'hunk';

export interface DiffLine {
  kind: DiffLineKind;
  text: string;
}

export interface DiffFile {
  path: string;
  additions: number;
  deletions: number;
  lines: DiffLine[];
}

const FILE_HEADER = /^diff --git a\/(.+?) b\/(.+)$/;
const HEADER_PREFIXES = [
  '--- ',
  '+++ ',
  'index ',
  'new file mode',
  'deleted file mode',
  'old mode',
  'new mode',
  'similarity index',
  'rename from',
  'rename to',
  'Binary files',
];

function lineKind(line: string): DiffLineKind {
  if (line.startsWith('@@')) return 'hunk';
  if (line.startsWith('+')) return 'added';
  if (line.startsWith('-')) return 'removed';
  return 'context';
}

/** A unified diff split into its files, each with its own lines and counts. */
export function splitDiff(diff: string): DiffFile[] {
  const files: DiffFile[] = [];
  let current: DiffFile | null = null;
  let inHunk = false;
  for (const line of diff.split('\n')) {
    const header = FILE_HEADER.exec(line);
    if (header) {
      current = { path: header[2], additions: 0, deletions: 0, lines: [] };
      files.push(current);
      inHunk = false;
      continue;
    }
    if (!current) continue;
    if (!inHunk && HEADER_PREFIXES.some((prefix) => line.startsWith(prefix))) continue;
    if (line.startsWith('\\ No newline')) continue;
    const kind = lineKind(line);
    if (kind === 'hunk') inHunk = true;
    if (!inHunk) continue;
    if (kind === 'added') current.additions += 1;
    if (kind === 'removed') current.deletions += 1;
    current.lines.push({ kind, text: line });
  }
  for (const file of files) {
    while (file.lines.at(-1)?.kind === 'context' && file.lines.at(-1)?.text === '') {
      file.lines.pop();
    }
  }
  return files;
}
