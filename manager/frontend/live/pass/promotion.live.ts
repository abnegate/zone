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
const POLICY = {
  occurrences: 4,
  distinctChats: 3,
  questionSimilarity: 0.88,
  answerSimilarity: 0.85,
  answerAgreement: 0.6,
  minimumAnswerCharacters: 40,
  maximumAnswerCharacters: 4_000,
} as const;

interface Exchange {
  id: string;
  chat: string;
  answer: string;
}

interface Pair {
  left: string;
  right: string;
  question: number;
  answer: number | null;
}

interface Readiness {
  exchanges: number;
  chats: number;
  cohesion: number;
  answers: number;
  agreement: number;
  promotable: boolean;
}

const punctuation = /^[!-\/:-@\[-^`{-~]+|[!-\/:-@\[-^`{-~]+$/g;

function normalize(text: string): string {
  return text
    .split(/\s+/)
    .map((word) => word.replace(punctuation, '').toLowerCase())
    .filter(Boolean)
    .join(' ');
}

/**
 * The exchanges the promotion job would load from these chats, and how its
 * bars judge them: the first reply after each embedded question, the leader's
 * cohesion, and the share of eligible answers that agree with the best one.
 */
function readiness(chats: string[]): Readiness {
  const inChats = chats.map((c) => `'${c}'`).join(',');
  const { exchanges, pairs } = JSON.parse(
    sql(
      `with exchange as (
        select question.id, question.chat_id, question.created_at, question_embedding.vector as question_vector, reply.content as answer, answer_embedding.vector as answer_vector
        from messages question
        join message_embeddings question_embedding on question_embedding.message_id = question.id
        left join lateral (select later.created_at, later.id from messages later where later.chat_id = question.chat_id and later.role = 'user' and (later.created_at, later.id) > (question.created_at, question.id) order by later.created_at, later.id limit 1) next_question on true
        join lateral (select candidate.id, candidate.content from messages candidate where candidate.chat_id = question.chat_id and candidate.role = 'assistant' and (candidate.created_at, candidate.id) > (question.created_at, question.id) and (next_question.created_at is null or (candidate.created_at, candidate.id) < (next_question.created_at, next_question.id)) order by candidate.created_at, candidate.id limit 1) reply on true
        left join message_embeddings answer_embedding on answer_embedding.message_id = reply.id
        where question.chat_id in (${inChats}) and question.role = 'user'
      )
      select json_build_object(
        'exchanges', (select coalesce(json_agg(json_build_object('id', id, 'chat', chat_id, 'answer', answer) order by created_at, id), '[]') from exchange),
        'pairs', (select coalesce(json_agg(json_build_object('left', a.id, 'right', b.id, 'question', 1 - (a.question_vector <=> b.question_vector), 'answer', case when a.answer_vector is not null and b.answer_vector is not null then 1 - (a.answer_vector <=> b.answer_vector) end)), '[]') from exchange a join exchange b on a.id <> b.id)
      )`,
    ).join('\n'),
  ) as { exchanges: Exchange[]; pairs: Pair[] };
  const pair = (left: Exchange, right: Exchange) =>
    pairs.find((p) => p.left === left.id && p.right === right.id);
  const [leader, ...rest] = exchanges;
  const cohesion = rest.reduce(
    (lowest, exchange) =>
      Math.min(lowest, pair(leader, exchange)?.question ?? 0),
    1,
  );
  const eligible = exchanges.filter((exchange) => {
    const length = [...exchange.answer.trim()].length;
    return (
      length >= POLICY.minimumAnswerCharacters &&
      length <= POLICY.maximumAnswerCharacters
    );
  });
  const agree = (left: Exchange, right: Exchange) => {
    const similarity = pair(left, right)?.answer;
    return similarity === null || similarity === undefined
      ? normalize(left.answer) === normalize(right.answer)
      : similarity >= POLICY.answerSimilarity;
  };
  const agreement =
    eligible.length < POLICY.occurrences
      ? 0
      : Math.max(
          ...eligible.map(
            (candidate) =>
              eligible.filter(
                (other) => other !== candidate && agree(candidate, other),
              ).length /
              (eligible.length - 1),
          ),
        );
  const distinct = new Set(exchanges.map((exchange) => exchange.chat)).size;
  return {
    exchanges: exchanges.length,
    chats: distinct,
    cohesion,
    answers: eligible.length,
    agreement,
    promotable:
      exchanges.length >= POLICY.occurrences &&
      distinct >= POLICY.distinctChats &&
      cohesion >= POLICY.questionSimilarity &&
      agreement >= POLICY.answerAgreement,
  };
}

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
    const promotable = await expect
      .poll(() => readiness(chats).promotable, {
        timeout: 120_000,
        intervals: [3_000],
      })
      .toBe(true)
      .then(() => true)
      .catch(() => false);
    const embeddings = sql(
      `select count(*) from message_embeddings where chat_id in (${chats.map((c) => `'${c}'`).join(',')})`,
    );
    const promotion = readiness(chats);
    const answered = replies.every((r) => r.includes('7070'));
    const seeded = promotable && answered;
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
      promotion,
      screenshots: ['133-question-asked-again.png'],
    });
    expect(promotable, JSON.stringify(promotion)).toBe(true);
    expect(answered, replies.join(' || ').slice(0, 600)).toBe(true);
  });
});
