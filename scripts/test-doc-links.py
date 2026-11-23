#!/usr/bin/env python3
"""Check that every relative link and #anchor in docs/*.md resolves to a file and a heading that exist."""

import os
import re
import sys
import unittest
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / 'docs'

PENDING: frozenset[Path] = frozenset()

FENCE = re.compile(r'^\s*(`{3,}|~{3,})')
HEADING = re.compile(r'^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$')
CODE_SPAN = re.compile(r'(`+)(.+?)\1', re.DOTALL)
INLINE_LINK = re.compile(r'!?\[(?:[^\[\]]|\[[^\[\]]*\])*\]\(\s*<?([^)\s>]*)>?(?:\s+(?:"[^"]*"|\'[^\']*\'))?\s*\)')
REFERENCE_DEFINITION = re.compile(r'^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+.*)?$', re.MULTILINE)
HTML_REFERENCE = re.compile(r'<(?:a|img)\b[^>]*?\b(?:href|src)\s*=\s*"([^"]*)"', re.IGNORECASE)
HTML_ANCHOR = re.compile(r'<a\b[^>]*?\b(?:id|name)\s*=\s*"([^"]+)"', re.IGNORECASE)
MARKDOWN_LINK_TEXT = re.compile(r'!?\[((?:[^\[\]]|\[[^\[\]]*\])*)\]\([^)]*\)')
HTML_TAG = re.compile(r'<[^>]+>')
SLUG_DROPPED = re.compile(r'[^\w\- ]')


@dataclass(frozen=True)
class Link:
    source: Path
    line: int
    target: str


@dataclass(frozen=True)
class Problem:
    link: Link
    reason: str

    def __str__(self) -> str:
        return f'{relative(self.link.source)}:{self.link.line}: {self.link.target}: {self.reason}'


@dataclass(frozen=True)
class Report:
    problems: tuple[Problem, ...]
    pending: tuple[Link, ...]
    stale: tuple[Path, ...]


def relative(path: Path, root: Path = ROOT) -> str:
    return path.relative_to(root).as_posix() if path.is_relative_to(root) else path.as_posix()


def without_fences(text: str) -> str:
    """Blanks fenced code blocks, keeping every newline so line numbers still match."""
    lines = []
    fence: str | None = None
    for line in text.split('\n'):
        opening = FENCE.match(line)
        if fence is None and opening:
            fence = opening.group(1)
            lines.append('')
        elif fence is not None:
            if opening and opening.group(1)[0] == fence[0] and len(opening.group(1)) >= len(fence):
                fence = None
            lines.append('')
        else:
            lines.append(line)
    return '\n'.join(lines)


def without_code(text: str) -> str:
    return CODE_SPAN.sub(lambda match: re.sub(r'[^\n]', ' ', match.group(0)), without_fences(text))


def slug(heading: str) -> str:
    """GitHub's heading anchor: the rendered text, lowercased, without punctuation, spaces as hyphens."""
    text = MARKDOWN_LINK_TEXT.sub(lambda match: match.group(1), heading)
    text = HTML_TAG.sub('', text).replace('`', '').replace('*', '')
    return SLUG_DROPPED.sub('', text.lower()).replace(' ', '-')


def anchors(text: str) -> frozenset[str]:
    seen: dict[str, int] = {}
    found = set()
    prose = without_fences(text)
    for line in prose.split('\n'):
        heading = HEADING.match(line)
        if not heading:
            continue
        base = slug(heading.group(2))
        count = seen.get(base, 0)
        seen[base] = count + 1
        found.add(base if count == 0 else f'{base}-{count}')
    found.update(HTML_ANCHOR.findall(without_code(text)))
    return frozenset(found)


def links(source: Path) -> Iterator[Link]:
    prose = without_code(source.read_text(encoding='utf-8'))
    for pattern in (INLINE_LINK, REFERENCE_DEFINITION, HTML_REFERENCE):
        for match in pattern.finditer(prose):
            yield Link(source, prose.count('\n', 0, match.start(1)) + 1, match.group(1))


def external(target: str) -> bool:
    return bool(urlsplit(target).scheme) or target.startswith('//')


def check(sources: Iterable[Path], root: Path = ROOT, pending: frozenset[Path] = PENDING) -> Report:
    cache: dict[Path, frozenset[str]] = {}

    def anchors_of(path: Path) -> frozenset[str]:
        if path not in cache:
            cache[path] = anchors(path.read_text(encoding='utf-8'))
        return cache[path]

    problems = []
    waiting = []
    for source in sources:
        for link in links(source):
            if not link.target or external(link.target):
                continue
            parts = urlsplit(link.target)
            path = Path(os.path.normpath(source.parent / unquote(parts.path))) if parts.path else source
            name = relative(path, root)
            if not path.exists():
                if Path(name) in pending:
                    waiting.append(link)
                else:
                    problems.append(Problem(link, f'{name} does not exist'))
                continue
            if not parts.fragment:
                continue
            if path.is_dir() or path.suffix.lower() != '.md':
                problems.append(Problem(link, f'{name} is not a Markdown file, so it has no #{parts.fragment}'))
                continue
            if unquote(parts.fragment) not in anchors_of(path):
                problems.append(Problem(link, f'{name} has no heading or anchor #{parts.fragment}'))
    stale = tuple(sorted(entry for entry in pending if (root / entry).exists()))
    return Report(tuple(problems), tuple(waiting), stale)


def notice(link: Link) -> None:
    message = f'{link.target} is pending: another change adds it, so this link is not checked yet'
    if os.environ.get('GITHUB_ACTIONS') == 'true':
        print(f'::notice file={relative(link.source)},line={link.line}::{message}')
    else:
        print(f'notice: {relative(link.source)}:{link.line}: {message}', file=sys.stderr)


class DocLinks(unittest.TestCase):
    def test_every_relative_link_and_anchor_in_the_docs_resolves(self) -> None:
        sources = sorted(DOCS.glob('*.md'))
        self.assertTrue(sources, f'no Markdown files in {DOCS}')
        report = check(sources)
        for link in report.pending:
            notice(link)
        self.assertEqual(
            [str(problem) for problem in report.problems],
            [],
            'Fix each link, or the heading it points at',
        )

    def test_every_pending_target_is_still_missing(self) -> None:
        stale = check(sorted(DOCS.glob('*.md'))).stale
        self.assertEqual(
            [entry.as_posix() for entry in stale],
            [],
            'These exist now, so remove them from PENDING and let their links be checked',
        )


class Slug(unittest.TestCase):
    def test_matches_github_anchors(self) -> None:
        for heading, expected in [
            ('Checking a license ID', 'checking-a-license-id'),
            ('Why the signer listens only to `order.paid`', 'why-the-signer-listens-only-to-orderpaid'),
            ("What's in scope", 'whats-in-scope'),
            ('Response and update targets (DRAFT, to be confirmed)', 'response-and-update-targets-draft-to-be-confirmed'),
            ('🔁 License renewal', '-license-renewal'),
            ('`SMTP_HOST`, `SMTP_PORT`', 'smtp_host-smtp_port'),
            ('[Linked](https://example.com) heading', 'linked-heading'),
            ('**Bold** move', 'bold-move'),
        ]:
            with self.subTest(heading=heading):
                self.assertEqual(slug(heading), expected)

    def test_numbers_repeated_headings(self) -> None:
        self.assertEqual(anchors('# Logs\n## Logs\n### Logs\n'), {'logs', 'logs-1', 'logs-2'})

    def test_ignores_headings_in_code_and_reads_html_anchors(self) -> None:
        text = '# Real `code`\n```sh\n# not a heading\n```\n   ~~~\n## Also not\n   ~~~\n<a id="custom"></a>\n'
        self.assertEqual(anchors(text), {'real-code', 'custom'})


class Check(unittest.TestCase):
    def setUp(self) -> None:
        directory = TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / 'docs').mkdir()
        (self.root / 'docs' / 'OTHER.md').write_text('# Other\n## Some section\n', encoding='utf-8')
        (self.root / 'README.md').write_text('# Readme\n', encoding='utf-8')

    def report(self, text: str, pending: frozenset[Path] = frozenset()) -> Report:
        source = self.root / 'docs' / 'PAGE.md'
        source.write_text(text, encoding='utf-8')
        return check([source], self.root, pending)

    def reasons(self, text: str) -> list[str]:
        return [problem.reason for problem in self.report(text).problems]

    def test_accepts_links_that_resolve(self) -> None:
        text = '\n'.join(
            [
                '# Page',
                '[same page](#page) [other](OTHER.md) [section](OTHER.md#some-section)',
                '[up](../README.md#readme) [external](https://example.com/missing) [mail](mailto:a@b.test)',
                '[split',
                'text](OTHER.md#other) ![image](OTHER.md)',
                '[reference]: OTHER.md#some-section',
                '<a href="OTHER.md#other">html</a>',
            ]
        )
        self.assertEqual(self.reasons(text), [])

    def test_reports_a_missing_file_and_a_missing_anchor_with_their_lines(self) -> None:
        problems = self.report('# Page\n[gone](GONE.md)\n\n[bad](OTHER.md#nope) [here](#nowhere)\n').problems
        self.assertEqual(
            [(problem.link.line, problem.link.target) for problem in problems],
            [(2, 'GONE.md'), (4, 'OTHER.md#nope'), (4, '#nowhere')],
        )

    def test_reports_an_anchor_into_a_file_that_is_not_markdown(self) -> None:
        (self.root / 'script.sh').write_text('#!/bin/sh\n', encoding='utf-8')
        self.assertEqual(len(self.reasons('[script](../script.sh#L1)\n')), 1)

    def test_skips_links_inside_code(self) -> None:
        self.assertEqual(self.reasons('`[gone](GONE.md)`\n```\n[gone](GONE.md)\n```\n'), [])

    def test_a_pending_target_is_a_notice_not_a_problem(self) -> None:
        report = self.report('[ha](HA.md) [section](HA.md#topology)\n', frozenset({Path('docs/HA.md')}))
        self.assertEqual(report.problems, ())
        self.assertEqual([link.target for link in report.pending], ['HA.md', 'HA.md#topology'])
        self.assertEqual(report.stale, ())

    def test_only_the_listed_target_is_pending(self) -> None:
        reasons = [problem.reason for problem in self.report('[gone](GONE.md)\n', frozenset({Path('docs/HA.md')})).problems]
        self.assertEqual(reasons, ['docs/GONE.md does not exist'])

    def test_a_pending_target_that_now_exists_is_checked_and_reported_stale(self) -> None:
        (self.root / 'docs' / 'HA.md').write_text('# HA\n', encoding='utf-8')
        report = self.report('[ha](HA.md#missing)\n', frozenset({Path('docs/HA.md')}))
        self.assertEqual(report.pending, ())
        self.assertEqual(len(report.problems), 1)
        self.assertEqual(report.stale, (Path('docs/HA.md'),))


if __name__ == '__main__':
    unittest.main()
