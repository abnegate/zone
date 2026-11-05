import { describe, expect, it } from 'bun:test';
import { splitDiff } from './diff';

const DIFF = [
  'diff --git a/src/old.rs b/src/new.rs',
  'similarity index 90%',
  'rename from src/old.rs',
  'rename to src/new.rs',
  'index 1..2 100644',
  '--- a/src/old.rs',
  '+++ b/src/new.rs',
  '@@ -1,3 +1,3 @@',
  ' fn main() {',
  '-    old();',
  '--- a removed comment line',
  '+    new();',
  ' }',
  '\\ No newline at end of file',
  'diff --git a/README.md b/README.md',
  'new file mode 100644',
  '--- /dev/null',
  '+++ b/README.md',
  '@@ -0,0 +1 @@',
  '+# Widgets',
  '',
].join('\n');

describe('splitDiff', () => {
  it('splits a unified diff into its files, each with its own lines and counts', () => {
    const files = splitDiff(DIFF);

    expect(files.map((file) => [file.path, file.additions, file.deletions])).toEqual([
      ['src/new.rs', 1, 2],
      ['README.md', 1, 0],
    ]);
    expect(files[0].lines.map((line) => line.kind)).toEqual([
      'hunk',
      'context',
      'removed',
      'removed',
      'added',
      'context',
    ]);
    expect(files[1].lines.at(-1)).toEqual({ kind: 'added', text: '+# Widgets' });
  });

  it('starts a new file at every header, however git wrote its path', () => {
    const files = splitDiff(
      [
        'diff --git "a/caf\\303\\251.txt" "b/caf\\303\\251.txt"',
        '--- "a/caf\\303\\251.txt"',
        '+++ "b/caf\\303\\251.txt"',
        '@@ -1 +1 @@',
        '-old',
        '+new',
        'diff --git a/docs/a b/c.md b/docs/a b/c.md',
        '--- a/docs/a b/c.md',
        '+++ b/docs/a b/c.md',
        '@@ -1 +1 @@',
        '+one',
        'diff --git a/gone.rs b/gone.rs',
        'deleted file mode 100644',
        '--- a/gone.rs',
        '+++ /dev/null',
        '@@ -1 +0,0 @@',
        '-fn gone() {}',
        'diff --git a/logo.png b/logo.png',
        'Binary files a/logo.png and b/logo.png differ',
      ].join('\n')
    );

    expect(files.map((file) => [file.path, file.additions, file.deletions])).toEqual([
      ['caf\\303\\251.txt', 1, 1],
      ['docs/a b/c.md', 1, 0],
      ['gone.rs', 0, 1],
      ['logo.png', 0, 0],
    ]);
    expect(files[3].lines).toEqual([]);
  });

  it('reads nothing from text that is not a diff', () => {
    expect(splitDiff('')).toEqual([]);
    expect(splitDiff('no files here\n+ or here')).toEqual([]);
  });
});
