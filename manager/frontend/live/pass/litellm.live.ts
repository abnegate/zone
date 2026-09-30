import { execFileSync } from 'node:child_process';
import {
  api,
  enabled,
  expect,
  newChat,
  record,
  send,
  settled,
  shot,
  signIn,
  sql,
  state,
  test,
  tokenFor,
} from './rig';

/**
 * Row 58 against a LiteLLM started from the repository's own templates
 * (litellm/config.yaml.template, router.json.template, entrypoint.sh) instead
 * of the compose stack's, so the stack is only read, never written. The rig's
 * LITELLM_HOST has to lead to that LiteLLM for the row: the organization's
 * LiteLLM host is saved below as a person would set it, but completions do not
 * read it. The second tenant's organization names a fast and a reasoning model,
 * and an Automatic chat asks a trivial question and then a hard one; LiteLLM's
 * log (LITELLM_LOG=INFO) says which deployment served each.
 */

const LITELLM = process.env.ZONE_LIVE_LITELLM_URL ?? '';
const KEY = process.env.ZONE_LIVE_LITELLM_KEY ?? '';
const CONTAINER = process.env.ZONE_LIVE_LITELLM_CONTAINER ?? '';
const FAST = process.env.ZONE_LIVE_LITELLM_FAST ?? 'llama3.2:3b';
const REASON = process.env.ZONE_LIVE_LITELLM_REASON ?? 'qwen3.8:27b-ctx32k';

function litellmLog(since: string): string {
  return execFileSync(
    'sh',
    ['-c', 'docker logs --since "$0" "$1" 2>&1', since, CONTAINER],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  );
}

function modelsIn(text: string): string[] {
  return [
    ...new Set(
      [...text.matchAll(/model[=:]\s*'?([A-Za-z0-9_.:\-/]+)/g)].map(
        (match) => match[1],
      ),
    ),
  ];
}

test.describe('LiteLLM routing', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.skip(
    !LITELLM || !KEY || !CONTAINER,
    'set ZONE_LIVE_LITELLM_URL, _KEY and _CONTAINER to a LiteLLM built from litellm/',
  );
  test.describe.configure({ timeout: 2_400_000 });

  test('58: automatic routing sends a trivial question to the fast model and a hard one to the reasoning model', async ({
    page,
  }) => {
    const tenant = state.intruder;
    const token = await tokenFor(tenant);
    const settingsPath = `/api/organizations/${tenant.organization.id}/settings/ai`;
    const saved = await api('PUT', settingsPath, {
      token,
      body: {
        provider: 'self_hosted',
        litellm_host: `${LITELLM}/v1`,
        litellm_key: KEY,
        model_fast: FAST,
        model_reasoning: REASON,
      },
    });
    expect(saved.status).toBe(200);
    try {
      await signIn(page, tenant);
      const since = new Date().toISOString();
      const chatId = await newChat(page, { model: 'Automatic' });
      await send(page, 'What is 2 plus 2? Answer with the number only.');
      await settled(page, 1, 900_000);
      await shot(page, '58-trivial-question');
      const afterTrivial = litellmLog(since);
      await send(
        page,
        'Prove step by step that the sum of the first n odd numbers is n squared, and analyze the edge cases of the proof.',
      );
      await settled(page, 2, 1_500_000);
      await shot(page, '58-hard-question');
      const afterHard = litellmLog(since);
      const trivialModels = modelsIn(afterTrivial);
      const hardModels = modelsIn(afterHard).filter(
        (name) => !trivialModels.includes(name),
      );
      const replies = sql(
        `select left(replace(message->>'content', E'\\n', ' '), 200) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'assistant' order by position`,
      );
      const routed =
        trivialModels.some((name) => name.includes(FAST)) &&
        hardModels.some((name) => name.includes(REASON));
      record(58, {
        result: routed ? 'WORKS' : 'FAILS',
        cause: routed
          ? undefined
          : `LiteLLM served ${trivialModels.join(',')} for the trivial turn and ${hardModels.join(',')} next`,
        litellm: LITELLM,
        fast: FAST,
        reasoning: REASON,
        chat_id: chatId,
        chat_model: sql(`select model_name from chats where id = '${chatId}'`),
        replies,
        litellm_models_after_trivial: trivialModels,
        litellm_models_new_after_hard: hardModels,
        litellm_log: afterHard
          .split('\n')
          .filter((line) => /completion\(\)|POST \/v1\/chat/.test(line))
          .slice(-6)
          .map((line) => line.slice(0, 200)),
        screenshots: ['58-trivial-question.png', '58-hard-question.png'],
      });
      expect(routed).toBe(true);
    } finally {
      await api('DELETE', settingsPath, { token });
    }
  });
});
