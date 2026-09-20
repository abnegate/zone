import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import {
  ask,
  enabled,
  expect,
  model,
  newChat,
  record,
  send,
  shot,
  signIn,
  sql,
  stamp,
  test,
} from './rig';

/**
 * Rows 33 to 37 and 39: the chat surface on the real model. What a turn did is
 * read from chat_entries (the stored transcript with its tool calls and
 * results) alongside what the console rendered.
 */

const FAST = 'llama3.2:3b';
const PERSONA_MODEL =
  process.env.ZONE_LIVE_PERSONA_MODEL ??
  'hf.co/Ttimofeyka/MistralRP-Noromaid-NSFW-Mistral-7B-GGUF:latest';
const ollama = process.env.OLLAMA_HOST ?? 'http://127.0.0.1:11434';

function entries(chatId: string): string[] {
  return sql(
    `select position || ' | ' || (message->>'role') || ' | ' || coalesce(message->'tool_calls'->0->'function'->>'name', '') || ' | ' || left(coalesce(message->>'content', ''), 160) from chat_entries where chat_id = '${chatId}' order by position`,
  );
}

function toolNames(chatId: string): string[] {
  return sql(
    `select c->'function'->>'name' from chat_entries, jsonb_array_elements(coalesce(nullif(message->'tool_calls', 'null'::jsonb), '[]'::jsonb)) c where chat_id = '${chatId}' order by position`,
  );
}

async function contextMeter(page: Page): Promise<string> {
  const group = page.getByRole('group', { name: 'Context usage' });
  if (!(await group.count())) return 'no context meter rendered';
  return (await group.innerText()).replace(/\s+/g, ' ');
}

async function ollamaUp(): Promise<boolean> {
  const response = await fetch(`${ollama}/api/tags`).catch(() => null);
  return Boolean(response && response.ok);
}

test.describe('chats', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 1_800_000 });

  test('33: chats are created, renamed, archived, deleted and searched; the model is chosen; agent mode gates the tools', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    // A fast-model chat and a reasoning-model chat, one turn each.
    const fastChat = await newChat(page, { model: FAST });
    const fastReply = await ask(page, `Reply with the single word pong-${s}.`, {
      replies: 1,
      timeout: 600_000,
    });
    await shot(page, '33-fast-model-chat');
    const reasonChat = await newChat(page, { model });
    await ask(
      page,
      `The secret word for this chat is zebra-${s}. Repeat it back in one sentence.`,
      { replies: 1, timeout: 600_000 },
    );
    await shot(page, '33-reasoning-model-chat');

    // Rename, archive, unarchive and delete the reasoning chat from the list.
    const active = page.locator('.chat-item.active');
    await active.hover();
    await active.locator('button[title="Rename"]').click();
    await page.locator('#chat-name').fill(`Renamed ${s}`);
    await page.getByRole('button', { name: 'Save name' }).click();
    await expect(
      page.locator('.chat-item', { hasText: `Renamed ${s}` }),
    ).toBeVisible({ timeout: 30_000 });
    await shot(page, '33-renamed');
    const renamedItem = page.locator('.chat-item', { hasText: `Renamed ${s}` });
    await renamedItem.hover();
    await renamedItem.locator('button[title=\"Archive\"]').click();
    await page.waitForTimeout(3_000);
    const archivedInDb = sql(
      `select archived from chats where id = '${reasonChat}'`,
    ).join(',');
    const stillListedAfterArchive = await page
      .locator('.chat-item', { hasText: `Renamed ${s}` })
      .count();
    await shot(page, '33-after-archive-click');
    if (stillListedAfterArchive > 0) {
      await page.reload();
      await page.waitForTimeout(2_000);
    }
    const listedAfterReload = await page
      .locator('.chat-item', { hasText: `Renamed ${s}` })
      .count();
    await page.getByRole('tab', { name: 'Archived' }).click();
    const archived = page.locator('.chat-item', { hasText: `Renamed ${s}` });
    await expect(archived).toBeVisible({ timeout: 30_000 });
    await shot(page, '33-archived');
    await archived.hover();
    await archived.locator('button[title="Unarchive"]').click();
    await page.waitForTimeout(3_000);
    const stillListedAfterUnarchive = await archived.count();
    const unarchivedInDb = sql(
      `select archived from chats where id = '${reasonChat}'`,
    ).join(',');
    if (stillListedAfterUnarchive > 0) {
      await page.reload();
      await page.waitForTimeout(2_000);
    }
    await page.getByRole('tab', { name: 'Active' }).click();
    await expect(
      page.locator('.chat-item', { hasText: `Renamed ${s}` }),
    ).toBeVisible({ timeout: 30_000 });

    // Search across chats for the word only the reasoning chat carries.
    await page.locator('[data-testid="chat-search-input"]').fill(`zebra-${s}`);
    const searchResponse = page
      .waitForResponse((r) => r.url().includes('/api/chats/search'), {
        timeout: 30_000,
      })
      .then((r) => `${r.status()} ${r.url().replace(/^.*\/api/, '/api')}`)
      .catch(() => 'no request seen');
    await page.locator('[data-testid="chat-search-input"]').press('Enter');
    const results = page.locator('[data-testid="search-result-item"]');
    const searchRequest = await searchResponse;
    await page.waitForTimeout(3_000);
    const searchHits = await results.count();
    const resultText = searchHits
      ? (await results.first().innerText()).replace(/\s+/g, ' ')
      : `no results (${searchRequest})`;
    await shot(page, '33-search-results');
    if (await page.locator('[data-testid="clear-search-btn"]').count())
      await page.locator('[data-testid="clear-search-btn"]').click();
    else await page.locator('[data-testid="chat-search-input"]').fill('');

    const toDelete = page.locator('.chat-item', { hasText: `Renamed ${s}` });

    await toDelete.hover();

    await toDelete.locator('button[title=\"Delete\"]').click();
    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Delete Chat');
    await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
    await expect(
      page.locator('.chat-item', { hasText: `Renamed ${s}` }),
    ).toHaveCount(0, { timeout: 30_000 });
    const deleted = sql(
      `select count(*) from chats where id = '${reasonChat}'`,
    ).join(',');
    await shot(page, '33-deleted');

    // Agent mode off: a request that needs a tool gets no tool; on: it does.
    const plainChat = await newChat(page, { model, agent: false });
    await ask(
      page,
      'Run the shell command uname -a on this machine and tell me the exact output.',
      { replies: 1, timeout: 600_000 },
    );
    const plainCalls = await page.locator('[data-testid="tool-call"]').count();
    const plainTools = toolNames(plainChat);
    await shot(page, '33-agent-off-no-tools');
    const agentChat = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    await ask(
      page,
      'Run the shell command uname -a on this machine and tell me the exact output.',
      { replies: 1, timeout: 600_000 },
    );
    const agentCalls = await page.locator('[data-testid="tool-call"]').count();
    const agentTools = toolNames(agentChat);
    await shot(page, '33-agent-on-tools');

    record(33, {
      result:
        /pong/i.test(fastReply) &&
        deleted === '0' &&
        plainCalls === 0 &&
        plainTools.length === 0 &&
        agentCalls > 0 &&
        agentTools.length > 0 &&
        stillListedAfterArchive === 0 &&
        stillListedAfterUnarchive === 0 &&
        searchHits > 0
          ? 'WORKS'
          : 'FAILS',
      cause:
        [
          stillListedAfterArchive === 0 && stillListedAfterUnarchive === 0
            ? ''
            : 'product: Archive and Unarchive change the chat on the server but the list being viewed keeps showing it until the page is reloaded',
          searchHits > 0
            ? ''
            : `product: the chat search shows no results; the console's request was ${searchRequest} (the server wants workspace_id)`,
        ]
          .filter(Boolean)
          .join('; ') || undefined,
      search_request: searchRequest,
      search_hits: searchHits,
      archived_in_db_after_click: archivedInDb,
      still_in_active_list_after_archive: stillListedAfterArchive,
      in_active_list_after_reload: listedAfterReload,
      still_in_archived_list_after_unarchive: stillListedAfterUnarchive,
      archived_in_db_after_unarchive: unarchivedInDb,
      fast_chat: fastChat,
      fast_reply: fastReply.slice(0, 120),
      renamed_and_deleted_chat: reasonChat,
      chat_rows_after_delete: deleted,
      search_first_result: resultText.slice(0, 200),
      agent_off_chat: plainChat,
      agent_off_tool_calls_rendered: plainCalls,
      agent_off_tool_calls_stored: plainTools,
      agent_on_chat: agentChat,
      agent_on_tool_calls_rendered: agentCalls,
      agent_on_tool_calls_stored: agentTools,
      catalog_evidence:
        'the server writes no catalog line; the stored transcript of the agent-off chat carries no tool calls while the agent-on chat does',
      screenshots: [
        '33-fast-model-chat.png',
        '33-reasoning-model-chat.png',
        '33-renamed.png',
        '33-after-archive-click.png',
        '33-archived.png',
        '33-search-results.png',
        '33-deleted.png',
        '33-agent-off-no-tools.png',
        '33-agent-on-tools.png',
      ],
    });
    expect(deleted).toBe('0');
    expect(plainTools).toEqual([]);
    expect(agentTools.length).toBeGreaterThan(0);
  });

  test('34: a streamed reply shows its reasoning, Stop ends a stream, and the context meter moves', async ({
    page,
  }) => {
    await signIn(page);
    const chatId = await newChat(page, { model });
    const before = await contextMeter(page);
    await ask(
      page,
      'Think it through carefully and explain step by step why 91 is not a prime number.',
      { replies: 1, timeout: 600_000 },
    );
    const reasoning = await page.locator('[data-testid="reasoning"]').count();
    const reasoningText = reasoning
      ? (
          await page.locator('[data-testid="reasoning"]').first().innerText()
        ).replace(/\s+/g, ' ')
      : '';
    const afterFirst = await contextMeter(page);
    await shot(page, '34-reasoning-block');

    await send(
      page,
      'Write a 2000 word essay about the history of the Auckland Harbour Bridge, in full.',
    );
    await expect(page.locator('.message-status')).toHaveCount(1, {
      timeout: 120_000,
    });
    await expect
      .poll(
        async () =>
          (await page.locator('.message-assistant').last().innerText()).length,
        { timeout: 300_000 },
      )
      .toBeGreaterThan(200);
    await page.getByRole('button', { name: 'Stop' }).click();
    await expect(page.locator('.message-status')).toHaveCount(0, {
      timeout: 60_000,
    });
    const partial = await page.locator('.message-assistant').last().innerText();
    await shot(page, '34-stopped-mid-stream');
    await page.waitForTimeout(3_000);
    const afterStop = await contextMeter(page);
    const stored = sql(
      `select count(*) from messages where chat_id = '${chatId}' and role = 'assistant'`,
    ).join(',');

    record(34, {
      result:
        reasoning > 0 &&
        partial.length > 100 &&
        partial.length < 12_000 &&
        afterFirst !== before
          ? 'WORKS'
          : 'FAILS',
      chat_id: chatId,
      reasoning_blocks: reasoning,
      reasoning_excerpt: reasoningText.slice(0, 200),
      partial_reply_length_after_stop: partial.length,
      context_meter_before: before,
      context_meter_after_first_turn: afterFirst,
      context_meter_after_stop: afterStop,
      assistant_messages_stored: stored,
      screenshots: ['34-reasoning-block.png', '34-stopped-mid-stream.png'],
    });
    expect(reasoning).toBeGreaterThan(0);
    expect(partial.length).toBeGreaterThan(100);
  });

  test('35: a character saved on a chat answers in persona', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    let chatId = '';
    let toggleVisible = false;
    for (const candidate of [PERSONA_MODEL, FAST, model]) {
      chatId = await newChat(page, { model: candidate });
      toggleVisible = await page
        .locator('[data-testid="character-toggle"]')
        .isVisible()
        .catch(() => false);
      if (toggleVisible) break;
    }
    const chosen = sql(
      `select model_name from chats where id = '${chatId}'`,
    ).join(',');
    if (!toggleVisible) {
      await shot(page, '35-no-character-control');
      record(35, {
        result: 'FAILS',
        cause:
          'product: no chat model on this rig shows the Character control (it appears only for models the server flags needs_character)',
        chat_id: chatId,
        models_tried: [PERSONA_MODEL, FAST, model],
        screenshots: ['35-no-character-control.png'],
      });
      expect(toggleVisible).toBe(true);
      return;
    }
    await page.locator('[data-testid="character-toggle"]').click();
    const prompt = `You are Captain Quill ${s}, a cheerful pirate captain. Stay in character, call the user "matey", and end every reply with the word Arr!`;
    await page.locator('#character-draft').fill(prompt);
    await shot(page, '35-character-editor');
    await page.getByRole('button', { name: 'Save character' }).click();
    await page.waitForTimeout(1_500);
    const reply = await ask(
      page,
      'Introduce yourself in two sentences and tell me what you do.',
      { replies: 1, timeout: 600_000 },
    );
    await shot(page, '35-persona-reply');
    const stored = sql(
      `select left(character::text, 200) from chats where id = '${chatId}'`,
    ).join(',');
    const systemEntries = sql(
      `select left(message->>'content', 200) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'system' order by position`,
    );
    const inPersona = /quill|matey|arr/i.test(reply);
    record(35, {
      result: inPersona ? 'WORKS' : 'FAILS',
      cause: inPersona
        ? undefined
        : `model: the reply did not carry the persona: ${reply.slice(0, 160)}`,
      chat_id: chatId,
      model_used: chosen,
      character_column: stored,
      system_entries_in_transcript: systemEntries,
      reply: reply.slice(0, 300),
      prompt_evidence:
        'the server writes no system prompt line; the character is read from chats.character and the stored transcript',
      screenshots: ['35-character-editor.png', '35-persona-reply.png'],
    });
    expect(inPersona).toBe(true);
  });

  test('36: an image and a text file are attached and read', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const dir = mkdtempSync(join(tmpdir(), 'zone-pass-'));
    const textFile = join(dir, `launch-${s}.txt`);
    writeFileSync(
      textFile,
      `Mission notes\n\nThe launch code for the ${s} mission is 4471-${s}.\nDo not share it.\n`,
    );
    const imageFile =
      process.env.ZONE_PASS_IMAGE ??
      join(
        process.env.ZONE_LIVE_WORK ?? '/tmp',
        '..',
        'prep',
        'comfy-direct',
        'image.png',
      );

    const chatId = await newChat(page, { model: 'Automatic' });
    const input = page.locator('input[type="file"][multiple]');
    await input.setInputFiles(imageFile);
    await expect(page.locator('.attachment-chip.has-thumb')).toBeVisible({
      timeout: 30_000,
    });
    const hint =
      (await page
        .locator('.message-form-hint')
        .innerText()
        .catch(() => '')) || '';
    await shot(page, '36-image-attached');
    const imageReply = await ask(
      page,
      'Describe what is in the attached image in one sentence.',
      { replies: 1, timeout: 900_000 },
    );
    await shot(page, '36-image-described');
    const loaded = (await (await fetch(`${ollama}/api/ps`)).json()) as {
      models?: { name: string }[];
    };

    await input.setInputFiles(textFile);
    await expect(
      page.locator('.attachment-chip', { hasText: `launch-${s}.txt` }),
    ).toBeVisible({ timeout: 30_000 });
    await shot(page, '36-text-attached');
    const textReply = await ask(
      page,
      'What launch code does the attached file mention? Answer with the code only.',
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '36-text-answered');
    const stored = entries(chatId);
    const codeSeen = textReply.includes(`4471-${s}`);
    const imageMentions =
      /lighthouse|sea|coast|storm|rock|tower|water|wave|cliff|sky/i.test(
        imageReply,
      );
    record(36, {
      result: codeSeen && imageMentions ? 'WORKS' : 'FAILS',
      cause:
        codeSeen && imageMentions
          ? undefined
          : `model: image reply "${imageReply.slice(0, 120)}", text reply "${textReply.slice(0, 120)}"`,
      chat_id: chatId,
      image_file: imageFile,
      starting_image_hint: hint,
      image_reply: imageReply.slice(0, 300),
      models_loaded_after_image_turn: (loaded.models ?? []).map((m) => m.name),
      text_reply: textReply.slice(0, 200),
      transcript: stored.slice(0, 6),
      note: '"Use as starting image" is exercised with the image edit weights in the media rows (row 38)',
      screenshots: [
        '36-image-attached.png',
        '36-image-described.png',
        '36-text-attached.png',
        '36-text-answered.png',
      ],
    });
    expect(codeSeen, textReply.slice(0, 200)).toBe(true);
    expect(imageMentions, imageReply.slice(0, 200)).toBe(true);
  });

  test('37: attaching a repository to a chat, and a code question answered from the source', async ({
    page,
  }) => {
    await signIn(page);
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const sourcesControl = await page
      .getByRole('button', { name: /^sources$/i })
      .count();
    const chip = await page
      .locator('.attachment-chip', { hasText: /repository/ })
      .count();
    await shot(page, '37-composer-no-sources-chip');
    const reply = await ask(
      page,
      'In the GitHub source called "Scratch repo" in this workspace, read check.sh and tell me exactly which script it runs. Cite the file you read.',
      { replies: 1, timeout: 900_000 },
    );
    const tools = toolNames(chatId);
    const citations = await page
      .locator('[data-testid="citation"]')
      .allInnerTexts();
    await shot(page, '37-code-question');
    const storedCitations = sql(
      `select left((metadata->'citations')::text, 400) from messages where chat_id = '${chatId}' and role = 'assistant' and metadata ? 'citations' order by created_at desc limit 1`,
    );
    record(37, {
      result: 'FAILS',
      cause:
        'product: the composer has no Sources chip and no way to attach a repository to a chat; the only "Sources" element is the read-only citations block on a reply',
      chat_id: chatId,
      sources_controls_in_composer: sourcesControl,
      repository_chips: chip,
      code_question_reply: reply.slice(0, 300),
      tools_the_agent_used: tools,
      citations_rendered: citations.map((c) =>
        c.replace(/\s+/g, ' ').slice(0, 120),
      ),
      citations_stored: storedCitations,
      screenshots: ['37-composer-no-sources-chip.png', '37-code-question.png'],
    });
    expect(
      sourcesControl + chip,
      'no repository attachment control exists',
    ).toBe(0);
  });

  test('39: an upstream model failure is reported, and the chat is usable after', async ({
    page,
  }) => {
    await signIn(page);
    const chatId = await newChat(page, { model });
    await send(
      page,
      'Write a 1500 word story about a lighthouse keeper and a storm, in full.',
    );
    await expect(page.locator('.message-status')).toHaveCount(1, {
      timeout: 120_000,
    });
    await expect
      .poll(
        async () =>
          (
            await page
              .locator('.message-assistant')
              .last()
              .innerText()
              .catch(() => '')
          ).length,
        { timeout: 300_000 },
      )
      .toBeGreaterThan(50);
    execFileSync('sh', [process.env.ZONE_PASS_OLLAMA_CTL ?? '', 'stop']);
    await expect.poll(ollamaUp, { timeout: 60_000 }).toBe(false);
    await expect(page.locator('.message-status')).toHaveCount(0, {
      timeout: 300_000,
    });
    await page.waitForTimeout(2_000);
    const alert = page.locator(
      '.messages-container [role="alert"], .chats-error[role="alert"]',
    );
    const alertText = (await alert.allInnerTexts())
      .join(' | ')
      .replace(/\s+/g, ' ');
    const thread = (
      await page.locator('.messages-container').innerText()
    ).replace(/\s+/g, ' ');
    await shot(page, '39-ollama-stopped-mid-turn');

    execFileSync('sh', [process.env.ZONE_PASS_OLLAMA_CTL ?? '', 'start']);
    await expect.poll(ollamaUp, { timeout: 120_000 }).toBe(true);
    const reply = await ask(page, 'Reply with the single word recovered.', {
      replies: 2,
      timeout: 600_000,
    });
    await shot(page, '39-chat-usable-after');
    const reported =
      /error|fail|unavailable|could not|lost|interrupt|stopped|connection/i.test(
        `${alertText} ${thread}`,
      );
    record(39, {
      result: reported && reply.trim().length > 0 ? 'WORKS' : 'FAILS',
      note: 'Judged on a reply arriving after Ollama is back; the model may answer the interrupted request again rather than the new one-word instruction',
      chat_id: chatId,
      failure_alert: alertText.slice(0, 300),
      thread_tail: thread.slice(-300),
      reply_after_restart: reply.slice(0, 120),
      screenshots: [
        '39-ollama-stopped-mid-turn.png',
        '39-chat-usable-after.png',
      ],
    });
    expect(
      reported,
      `nothing reported the failure: ${thread.slice(-200)}`,
    ).toBe(true);
    expect(reply.trim().length, 'no reply after the restart').toBeGreaterThan(0);
  });
});
