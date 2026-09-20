import { describe, expect, it } from 'bun:test';
import { toPlainText } from './plainText';

describe('toPlainText', () => {
  it('reads a server-collapsed markdown list as one line of prose', () => {
    expect(
      toPlainText(
        'Here are the documents in the wiki: ⏎ - **Deployment checklist mua9iw0u1qg** — knowledge base note [doc:b9655d] ⏎ - **ch'
      )
    ).toBe(
      'Here are the documents in the wiki: Deployment checklist mua9iw0u1qg — knowledge base note [doc:b9655d] ch'
    );
  });

  it('drops emphasis, headings, quotes, code and link syntax but keeps the words', () => {
    expect(
      toPlainText(
        '# Status\n> **No — it did not succeed.**\n1. Run `check.sh` on _main_\n2. See [the run](https://ci.test/run/1) and ![shot](a.png)\n---\n```sh\necho done\n```'
      )
    ).toBe('Status No — it did not succeed. Run check.sh on main See the run and shot echo done');
  });

  it('leaves identifiers and arithmetic that only look like markdown alone', () => {
    expect(toPlainText('Wrote watch_mua7y7fe142.txt where 2 * 3 * 4 = 24')).toBe(
      'Wrote watch_mua7y7fe142.txt where 2 * 3 * 4 = 24'
    );
  });

  it('collapses runs of whitespace and returns nothing for an empty snippet', () => {
    expect(toPlainText('  one \r\n\n two  ')).toBe('one two');
    expect(toPlainText('')).toBe('');
  });
});
