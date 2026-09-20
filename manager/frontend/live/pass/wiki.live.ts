import {
  ask,
  enabled,
  expect,
  model,
  newChat,
  record,
  shot,
  signIn,
  sql,
  stamp,
  state,
  test,
} from './rig';

/**
 * Rows 54 and 55: the Wiki page and the Context Search page, with the agent
 * citing a wiki entry in chat.
 */

test.describe('wiki and search', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 1_200_000 });

  test('54: knowledge entries are added as text and URL, indexed, refreshed, searched, cited and deleted', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    page.on('dialog', (d) => d.accept());
    await page.goto('/wiki');

    async function open(
      kind: RegExp,
      value: string,
      title: string,
      tags: string[],
    ): Promise<void> {
      await page.getByRole('button', { name: /add knowledge/i }).click();
      const wizard = page.getByRole('dialog');
      await expect(wizard).toContainText('Add Knowledge Entry');
      await wizard.getByRole('button', { name: kind }).click();
      await wizard.getByRole('button', { name: 'Next' }).click();
      await wizard.locator('#knowledge-content').fill(value);
      await wizard.getByRole('button', { name: 'Next' }).click();
      await wizard.locator('#knowledge-title').fill(title);
      for (const tag of tags) {
        await wizard.locator('#knowledge-tags').fill(tag);
        await wizard.locator('#knowledge-tags').press('Enter');
      }
      await shot(
        page,
        `54-wizard-${title.toLowerCase().replace(/\W+/g, '-').slice(0, 24)}`,
      );
      await wizard.getByRole('button', { name: 'Create Entry' }).click();
      await expect(wizard).toBeHidden({ timeout: 30_000 });
    }

    const textTitle = `Harbour bridge facts ${s}`;
    await open(
      /Text Content/,
      `The Auckland Harbour Bridge opened in 1959 and gained its clip-on lanes in 1969. Marker ${s}.`,
      textTitle,
      ['history', `pass-${s}`],
    );
    const textCard = page.locator('.knowledge-card', { hasText: textTitle });
    await expect(textCard).toBeVisible({ timeout: 30_000 });
    const notIndexedAtFirst = await textCard.getByText('Not indexed').count();
    await shot(page, '54-text-entry-card');

    const urlTitle = `Example page ${s}`;
    await open(/URL \/ Web Page/, 'https://example.com/', urlTitle, ['web']);
    const urlCard = page.locator('.knowledge-card', { hasText: urlTitle });
    await expect(urlCard).toBeVisible({ timeout: 30_000 });
    const urlRow = () =>
      sql(
        `select coalesce(source_url, '') || ' | ' || left(replace(coalesce(content, ''), E'\\n', ' '), 60) || ' | ' || coalesce(last_fetched_at::text, '') || ' | ' || coalesce(last_fetch_error, '') from knowledge_entries where title = '${urlTitle}'`,
      ).join(',');
    const urlBadge = (
      await urlCard.locator('.ui-badge, .knowledge-card-type').allInnerTexts()
    )
      .join(' ')
      .trim();
    const refresh = urlCard.locator('button[aria-label="Refresh URL content"]');
    const refreshable = (await refresh.count()) > 0;
    const fetchedBefore = urlRow();
    if (refreshable) {
      await expect
        .poll(urlRow, { timeout: 120_000, intervals: [3_000] })
        .not.toMatch(/\| \| $/);
      await refresh.click();
      await page.waitForTimeout(5_000);
    }
    const fetchedAfter = urlRow();
    await shot(page, '54-url-entry-refreshed');

    await expect
      .poll(async () => textCard.getByText('Not indexed').count(), {
        timeout: 300_000,
        intervals: [5_000],
      })
      .toBe(0);
    await page.reload();
    await page.waitForTimeout(1_500);
    const indexedNow = await page
      .locator('.knowledge-card', { hasText: textTitle })
      .getByText('Not indexed')
      .count();
    const embeddings = sql(
      `select count(*) from knowledge_embeddings where knowledge_entry_id in (select id from knowledge_entries where title = '${textTitle}')`,
    ).join(',');

    await page.getByLabel('Search knowledge').fill(`Harbour bridge facts ${s}`);
    await page.waitForTimeout(800);
    const searched = await page.locator('.knowledge-card').count();
    await shot(page, '54-search-knowledge');

    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const reply = await ask(
      page,
      `When did the Auckland Harbour Bridge open and when did it get its clip-on lanes? Answer from the workspace knowledge base (the entry with marker ${s}) and cite it.`,
      { replies: 1, timeout: 600_000 },
    );
    const citations = (
      await page.locator('[data-testid="citation"]').allInnerTexts()
    ).map((c) => c.replace(/\s+/g, ' '));
    await shot(page, '54-agent-cites-entry');

    await page.goto('/wiki');
    await page
      .locator('.knowledge-card', { hasText: urlTitle })
      .locator('button[aria-label="Delete entry"]')
      .click();
    await expect(
      page.locator('.knowledge-card', { hasText: urlTitle }),
    ).toHaveCount(0, { timeout: 30_000 });
    const deleted = sql(
      `select count(*) from knowledge_entries where title = '${urlTitle}'`,
    ).join(',');
    await shot(page, '54-url-entry-deleted');

    record(54, {
      result:
        indexedNow === 0 &&
        Number(embeddings) > 0 &&
        searched >= 1 &&
        citations.some((c) => c.includes(textTitle)) &&
        deleted === '0' &&
        refreshable &&
        !/^ \|/.test(fetchedAfter)
          ? 'WORKS'
          : 'FAILS',
      cause: refreshable
        ? undefined
        : `product: the URL / Web Page kind is saved as a text entry (badge "${urlBadge}", source_url empty, never fetched): ${fetchedBefore}`,
      chat_id: chatId,
      url_entry_badge: urlBadge,
      url_entry_refresh_button: refreshable,
      not_indexed_badge_at_first: notIndexedAtFirst,
      not_indexed_badge_later: indexedNow,
      embeddings_for_text_entry: embeddings,
      url_fetched_before_refresh: fetchedBefore,
      url_fetched_after_refresh: fetchedAfter,
      search_matches: searched,
      agent_reply: reply.slice(0, 200),
      citations,
      url_entry_rows_after_delete: deleted,
      screenshots: [
        '54-text-entry-card.png',
        '54-url-entry-refreshed.png',
        '54-search-knowledge.png',
        '54-agent-cites-entry.png',
        '54-url-entry-deleted.png',
      ],
    });
    expect(indexedNow).toBe(0);
    expect(
      citations.some((c) => c.includes(textTitle)),
      citations.join(' || '),
    ).toBe(true);
    expect(deleted).toBe('0');
  });

  test('55: Context Search in hybrid, semantic and keyword modes, filtered by source, with relevance labels', async ({
    page,
  }) => {
    await signIn(page);
    const sources = sql(
      `select name || ' | ' || source_type || ' | ' || is_active from sources where workspace_id = '${state.owner.workspace.id}' order by created_at`,
    );
    await page.goto('/search');
    if (sources.length)
      await expect(page.locator('.source-pill').first()).toBeVisible({
        timeout: 60_000,
      });
    const pills = await page.locator('.source-pill').allInnerTexts();
    const results: Record<
      string,
      { count: number; first: string; labels: string[] }
    > = {};
    for (const [mode, query, pill] of [
      ['Hybrid', 'widget area check', 'Scratch repo'],
      [
        'Semantic',
        'a script that verifies a small python module',
        'Scratch repo',
      ],
      ['Keyword', 'widget', null],
    ] as const) {
      await page.getByRole('tab', { name: mode }).click();
      for (const active of await page.locator('.source-pill.active').all())
        await active.click();
      if (pill) {
        const target = page.locator('.source-pill', { hasText: pill });
        if (await target.count()) await target.click();
      }
      await page.getByPlaceholder('Search your knowledge base...').fill(query);
      const response = page
        .waitForResponse(
          (r) =>
            r.url().includes('/api/context/search') ||
            r.url().includes('/search'),
          { timeout: 60_000 },
        )
        .catch(() => null);
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      const seen = await response;
      await page.waitForTimeout(3_000);
      const cards = page.locator('.result-card');
      const count = await cards.count();
      const labels = await cards.locator('.ui-badge').allInnerTexts();
      results[`${mode}: ${query}${pill ? ` (source ${pill})` : ''}`] = {
        count,
        first: count
          ? (await cards.first().innerText()).replace(/\s+/g, ' ').slice(0, 200)
          : `no results (${
              seen
                ? `${seen.status()} ${seen
                    .url()
                    .replace(/^.*\/api/, '/api')
                    .slice(0, 120)}`
                : 'no request seen'
            })`,
        labels: labels.slice(0, 6),
      };
      await shot(page, `55-${mode.toLowerCase()}`);
    }
    const labelled = Object.values(results).flatMap((r) => r.labels);
    record(55, {
      result:
        Object.values(results).every((r) => r.count > 0) &&
        labelled.some((l) => /relevant|semantic|keyword/i.test(l))
          ? 'WORKS'
          : 'FAILS',
      sources_in_workspace: sources,
      source_pills: pills,
      results,
      note: 'Context Search covers indexed sources (zone_context content items); wiki entries have their own search on the Wiki page and the search_knowledge tool (rows 46 and 54). The relevance badge reads "<n>% semantic" or "Keyword match" when the server sends those scores, and "Highly relevant" or "Relevant" only when it sends neither',
      screenshots: ['55-hybrid.png', '55-semantic.png', '55-keyword.png'],
    });
    expect(
      Object.values(results).every((r) => r.count > 0),
      JSON.stringify(results),
    ).toBe(true);
  });
});
