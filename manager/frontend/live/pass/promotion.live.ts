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
 * Feature 133, first half: the same question is asked in four chats, three
 * of them separate, and answered alike, which is what the answer-promotion
 * job clusters. The answer comes from a knowledge entry the lane writes first:
 * asked to repeat a claim nothing in the workspace supports, the model refuses
 * in different words each time, which is no answer to promote.
 * The job first runs a whole six-hour period after the server starts (plus up
 * to a tenth of it), so the promotion itself is read from knowledge_entries
 * once the server has been up that long, not here.
 */

const FACT =
  'The Borealis build cache listens on port 7070 and keeps build artifacts for 14 days.';
const QUESTION =
  'Which port does the Borealis build cache listen on, and how long does it keep build artifacts?';
const OCCURRENCES = 4;
const DISTINCT_CHATS = 3;

test.describe('answer promotion', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 1_800_000 });

  test('133: a question asked in several chats is answered alike', async ({
    page,
  }) => {
    const s = stamp();
    const entry = await api('POST', '/api/knowledge', {
      token: await ownerToken(),
      body: {
        workspace_id: state.owner.workspace.id,
        title: `Borealis build cache ${s}`,
        category: 'reference',
        tags: ['infrastructure'],
        content: FACT,
      },
    });
    expect(entry.status).toBe(201);
    await signIn(page);
    const chats: string[] = [];
    const replies: string[] = [];
    for (let turn = 0; turn < 3; turn += 1) {
      chats.push(await newChat(page, { model }));
      replies.push(await ask(page, QUESTION, { replies: 1, timeout: 600_000 }));
    }
    replies.push(await ask(page, QUESTION, { replies: 2, timeout: 600_000 }));
    await shot(page, '133-question-asked-again');
    const inChats = chats.map((c) => `'${c}'`).join(',');
    const embeddedQuestions = () => {
      const [questions = '0', distinct = '0'] = (
        sql(
          `select count(*), count(distinct m.chat_id) from messages m join message_embeddings e on e.message_id = m.id where m.chat_id in (${inChats}) and m.role = 'user'`,
        )[0] ?? ''
      ).split('|');
      return { questions: Number(questions), chats: Number(distinct) };
    };
    const embedded = await expect
      .poll(
        () => {
          const { questions, chats: distinct } = embeddedQuestions();
          return questions >= OCCURRENCES && distinct >= DISTINCT_CHATS;
        },
        { timeout: 120_000, intervals: [3_000] },
      )
      .toBe(true)
      .then(() => true)
      .catch(() => false);
    const embeddings = sql(
      `select count(*) from message_embeddings where chat_id in (${inChats})`,
    );
    const answered = replies.every((r) => r.includes('7070'));
    const seeded = embedded && answered;
    record(133.1, {
      list: 'features',
      feature: 'Answer promotion: exchanges seeded',
      result: seeded ? 'SEEDED' : 'FAILS',
      stamp: s,
      knowledge_entry: (entry.body as { id?: string }).id,
      workspace_id: state.owner.workspace.id,
      chats,
      replies: replies.map((r) => r.replace(/\s+/g, ' ').slice(0, 200)),
      message_embeddings: embeddings,
      question_embeddings: embeddedQuestions(),
      screenshots: ['133-question-asked-again.png'],
    });
    expect(embedded, JSON.stringify(embeddedQuestions())).toBe(true);
    expect(answered, replies.join(' || ').slice(0, 600)).toBe(true);
  });
});
