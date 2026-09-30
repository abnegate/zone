import {
  api,
  ask,
  enabled,
  expect,
  model,
  newChat,
  ownerToken,
  record,
  shot,
  signIn,
  sql,
  stamp,
  state,
  test,
} from './rig';

/**
 * Feature 92: a knowledge entry with category "skill" is listed in an agent
 * chat's prompt by title, id and trigger, and the agent opens it with
 * read_document. The procedure's body carries a code word the index line does
 * not, so a reply that names it had to read the skill; which tools ran is read
 * from the stored transcript.
 */

test.describe('workspace skills', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 1_200_000 });

  test('92: a skill is offered to an agent chat and read with read_document', async ({
    page,
  }) => {
    const s = stamp();
    const word = `harbour-${s}`;
    const token = await ownerToken();
    const created = await api('POST', '/api/knowledge', {
      token,
      body: {
        workspace_id: state.owner.workspace.id,
        title: `Release freeze ${s}`,
        category: 'skill',
        tags: ['release'],
        content: [
          '---',
          `name: release-freeze-${s}`,
          'description: Use when someone asks whether they may deploy or release to production on a given day.',
          '---',
          '',
          '# Release freeze',
          '',
          'Production deploys are frozen every Friday from 14:00 NZT until Monday 09:00 NZT.',
          `During the freeze a release needs the approval code word ${word}, posted in the release channel before deploying.`,
        ].join('\n'),
      },
    });
    expect(created.status).toBe(201);
    const skillId = (created.body as { id: string }).id;

    await signIn(page);
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const reply = await ask(
      page,
      "May I deploy to production this Friday at 3pm? Follow this workspace's written procedure for deploys and tell me exactly what it requires, including any code word.",
      { replies: 1, timeout: 900_000 },
    );
    await shot(page, '92-skill-read-in-chat');

    const calls = sql(
      `select c->'function'->>'name' || ' ' || left(c->'function'->>'arguments', 200) from chat_entries, jsonb_array_elements(coalesce(nullif(message->'tool_calls', 'null'::jsonb), '[]'::jsonb)) c where chat_id = '${chatId}' order by position`,
    );
    const readSkill = calls.filter(
      (call) => call.startsWith('read_document') && call.includes(skillId),
    );
    const discovery = calls
      .slice(
        0,
        calls.findIndex((call) => call.startsWith('read_document')),
      )
      .filter((call) => /^(list_documents|search_knowledge)/.test(call));
    const skillRows = sql(
      `select category || ' | ' || is_active from knowledge_entries where id = '${skillId}'`,
    );
    const ok = readSkill.length > 0 && reply.includes(word);
    record(92, {
      list: 'features',
      feature: 'Workspace skills index',
      result: ok ? 'WORKS' : 'FAILS',
      cause: ok
        ? undefined
        : readSkill.length === 0
          ? `model: no read_document call on the skill (calls: ${calls.join(' | ').slice(0, 300)})`
          : 'the skill was read but the reply did not carry its code word',
      skill_id: skillId,
      skill_row: skillRows,
      chat_id: chatId,
      calls,
      read_document_on_skill: readSkill,
      searched_or_listed_before_reading: discovery,
      reply: reply.slice(0, 400),
      screenshots: ['92-skill-read-in-chat.png'],
    });
    expect(readSkill.length, calls.join(' | ')).toBeGreaterThan(0);
    expect(reply).toContain(word);
  });
});
