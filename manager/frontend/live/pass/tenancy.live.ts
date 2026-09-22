import {
  api,
  enabled,
  expect,
  record,
  shot,
  signIn,
  sql,
  state,
  test,
  tokenFor,
} from './rig';

/**
 * Row 56: signed in as the second tenant, nothing of the first tenant's chats
 * or wiki is listed or reachable by id. The existing tenancy lane covers tasks,
 * theme, AI settings and the run stream; this is the manual check on chats and
 * wiki the checklist adds.
 */

test.describe('tenancy', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 600_000 });

  test("56: the second tenant sees and reaches none of the first tenant's chats and wiki entries", async ({
    page,
  }) => {
    // Row 6 made the second tenant a member of the first tenant's organization
    // and workspace; a tenant is a stranger, so that membership is removed
    // (the rows are cleared outright, as the API's soft delete leaves them
    // counted) before anything is read.
    const removed = sql(
      `delete from workspace_members where workspace_id = '${state.owner.workspace.id}' and user_id = '${state.intruder.user.id}' returning role`,
    ).concat(
      sql(
        `delete from organization_members where organization_id = '${state.owner.organization.id}' and user_id = '${state.intruder.user.id}' returning role`,
      ),
    );
    const ownerChats = sql(
      `select id from chats where workspace_id = '${state.owner.workspace.id}' order by created_at desc limit 3`,
    );
    const ownerEntries = sql(
      `select id from knowledge_entries where workspace_id = '${state.owner.workspace.id}' order by created_at desc limit 3`,
    );
    expect(ownerChats.length, 'the first tenant has chats').toBeGreaterThan(0);
    expect(
      ownerEntries.length,
      'the first tenant has wiki entries',
    ).toBeGreaterThan(0);

    await signIn(page, state.intruder);
    await page.goto('/chats');
    await page.waitForTimeout(2_000);
    const chatList = (await page.locator('.chat-item').allInnerTexts()).join(
      ' | ',
    );
    await shot(page, '56-intruder-chats');
    await page.goto(`/chats?id=${ownerChats[0]}`);
    await page.waitForTimeout(3_000);
    const byIdText = (await page.locator('main').innerText()).replace(
      /\s+/g,
      ' ',
    );
    const byIdMessages = await page
      .locator('.message-user, .message-assistant')
      .count();
    await shot(page, '56-intruder-owner-chat-by-id');

    await page.goto('/wiki');
    await page.waitForTimeout(2_000);
    const wikiCards = await page.locator('.knowledge-card').count();
    const wikiText = (await page.locator('main').innerText()).replace(
      /\s+/g,
      ' ',
    );
    await shot(page, '56-intruder-wiki');

    const token = await tokenFor(state.intruder);
    const chat = await api('GET', `/api/chats/${ownerChats[0]}`, { token });
    const messages = await api('GET', `/api/chats/${ownerChats[0]}/messages`, {
      token,
    });
    const entry = await api('GET', `/api/knowledge/${ownerEntries[0]}`, {
      token,
    });
    const list = await api(
      'GET',
      `/api/knowledge?workspace_id=${state.owner.workspace.id}`,
      { token },
    );
    const ownerChatTitles = sql(
      `select title from chats where id in ('${ownerChats.join("','")}')`,
    );
    const leaked = ownerChatTitles.some((t) => t && chatList.includes(t));
    const refused = [
      chat.status,
      messages.status,
      entry.status,
      list.status,
    ].every((s) => [401, 403, 404].includes(s));
    record(56, {
      result:
        !leaked && byIdMessages === 0 && wikiCards === 0 && refused
          ? 'WORKS'
          : 'FAILS',
      intruder_chat_list: chatList.slice(0, 200),
      owner_chat_by_id_page: byIdText.slice(0, 200),
      owner_chat_by_id_messages_rendered: byIdMessages,
      intruder_wiki_cards: wikiCards,
      intruder_wiki_text: wikiText.slice(0, 160),
      api: {
        chat: chat.status,
        messages: messages.status,
        entry: entry.status,
        owner_workspace_list: list.status,
      },
      memberships_removed_first: removed,
      note: "With the row 6 membership still in place the second tenant could open the first tenant's chat by id and list its wiki through the API (evidence.jsonl earlier row 56 line): workspace members share the workspace's chats and entries by design; a stranger is what this row checks.",
      existing_lane:
        'live/tenancy.live.ts passed 3 of 3 against this rig (tasks, theme, AI settings, run stream)',
      screenshots: [
        '56-intruder-chats.png',
        '56-intruder-owner-chat-by-id.png',
        '56-intruder-wiki.png',
      ],
    });
    expect(leaked).toBe(false);
    expect(byIdMessages).toBe(0);
    expect(wikiCards).toBe(0);
    expect(
      refused,
      JSON.stringify({
        chat: chat.status,
        messages: messages.status,
        entry: entry.status,
        list: list.status,
      }),
    ).toBe(true);
  });
});
