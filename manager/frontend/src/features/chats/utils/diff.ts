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

const FILE_START = 'diff --git ';
const NEW_PATH = '+++ ';
const OLD_PATH = '--- ';
const NO_FILE = '/dev/null';
const NO_NEWLINE = '\\ No newline';
const UNQUOTED_HEADER = /^a\/(.+) b\/\1$/;

/** A path as git writes it in a header: quoted when it holds unusual bytes, and behind `a/` or `b/`. */
function headerPath(raw: string): string | null {
  const trimmed = raw.trim().replace(/\t.*$/, '');
  if (trimmed === NO_FILE) return null;
  const unquoted =
    trimmed.startsWith('"') && trimmed.endsWith('"') ? trimmed.slice(1, -1) : trimmed;
  return unquoted.replace(/^[ab]\//, '');
}

function startPath(header: string): string {
  return UNQUOTED_HEADER.exec(header)?.[1] ?? header;
}

function lineKind(line: string): DiffLineKind {
  if (line.startsWith('@@')) return 'hunk';
  if (line.startsWith('+')) return 'added';
  if (line.startsWith('-')) return 'removed';
  return 'context';
}

/**
 * A unified diff split into its files, each with its own lines and counts.
 * Every `diff --git` line starts a new file, and the file is named by its
 * `+++` path, or its `---` path when it was deleted.
 */
export function splitDiff(diff: string): DiffFile[] {
  const files: DiffFile[] = [];
  let current: DiffFile | null = null;
  let inHunk = false;
  for (const line of diff.split('\n')) {
    if (line.startsWith(FILE_START)) {
      current = {
        path: startPath(line.slice(FILE_START.length)),
        additions: 0,
        deletions: 0,
        lines: [],
      };
      files.push(current);
      inHunk = false;
      continue;
    }
    if (!current || line.startsWith(NO_NEWLINE)) continue;
    const kind = lineKind(line);
    if (!inHunk) {
      if (line.startsWith(NEW_PATH)) {
        current.path = headerPath(line.slice(NEW_PATH.length)) ?? current.path;
      } else if (line.startsWith(OLD_PATH)) {
        const old = headerPath(line.slice(OLD_PATH.length));
        if (old) current.path = old;
      }
      if (kind !== 'hunk') continue;
    }
    inHunk = true;
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
