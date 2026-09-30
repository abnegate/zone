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
  stamp,
  state,
  type Tenant,
  test,
  tokenFor,
} from './rig';

/**
 * Row 58 against a LiteLLM started from the repository's own templates
 * (litellm/config.yaml.template, router.json.template, entrypoint.sh) instead
 * of the compose stack's, so the stack is only read, never written. The rig's
 * LITELLM_HOST has to lead to that LiteLLM for the row: the organization's
 * LiteLLM host is saved below as a person would set it, but completions do not
 * read it. The second tenant opens a throwaway organization for the row, which
 * names a fast and a reasoning model and is deleted afterwards, so no shared
 * tenant's settings are ever changed. An Automatic chat asks a trivial question
 * and then a hard one; LiteLLM's log (LITELLM_LOG=INFO) says which deployment
 * served each.
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

/**
 * Earlier revisions of this lane backed the second tenant's own settings up in
 * this table around the row. An interrupted run could leave that tenant holding
 * the lane's settings, so a leftover backup is put back once and the table
 * dropped, in one statement.
 */
const RETIRE_BACKUP = `do $$
begin
  if to_regclass('live_pass_ai_settings_backup') is not null then
    delete from organization_ai_settings s using live_pass_ai_settings_backup b where s.organization_id = b.organization_id;
    insert into organization_ai_settings select (jsonb_populate_record(null::organization_ai_settings, settings)).* from live_pass_ai_settings_backup where settings is not null;
    drop table live_pass_ai_settings_backup;
  end if;
end $$`;

async function throwawayTenant(owner: Tenant, token: string): Promise<Tenant> {
  const s = stamp();
  const created = await api('POST', '/api/organizations', {
    token,
    body: {
      name: `LiteLLM routing ${s}`,
      slug: `litellm-routing-${s}`,
      description: 'throwaway tenant for live row 58',
    },
  });
  expect(created.status, JSON.stringify(created.body)).toBe(201);
  const { organization } = created.body as {
    organization: Tenant['organization'];
  };
  const space = await api(
    'POST',
    `/api/organizations/${organization.id}/workspaces`,
    {
      token,
      body: { name: 'Routing', slug: 'routing', description: 'live row 58' },
    },
  );
  expect(space.status, JSON.stringify(space.body)).toBe(201);
  const { workspace } = space.body as { workspace: Tenant['workspace'] };
  return { ...owner, organization, workspace };
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
    sql(RETIRE_BACKUP);
    const token = await tokenFor(state.intruder);
    const tenant = await throwawayTenant(state.intruder, token);
    const organizationPath = `/api/organizations/${tenant.organization.id}`;
    try {
      const saved = await api('PUT', `${organizationPath}/settings/ai`, {
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
      await signIn(page, tenant);
      const since = new Date().toISOString();
      const chatId = await newChat(page, { model: 'Automatic' });
      await send(page, 'What is 2 plus 2? Answer with the number only.');
      await settled(page, 1, 900_000);
      await shot(page, '58-trivial-question');
      const afterTrivial = litellmLog(since);
      const boundary = new Date().toISOString();
      await send(
        page,
        'Prove step by step that the sum of the first n odd numbers is n squared, and analyze the edge cases of the proof.',
      );
      await settled(page, 2, 1_500_000);
      await shot(page, '58-hard-question');
      const afterHard = litellmLog(boundary);
      const trivialModels = modelsIn(afterTrivial);
      const hardModels = modelsIn(afterHard);
      const replies = sql(
        `select left(replace(message->>'content', E'\\n', ' '), 200) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'assistant' order by position`,
      );
      const routed =
        trivialModels.some((name) => name.includes(FAST)) &&
        !trivialModels.some((name) => name.includes(REASON)) &&
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
        litellm_log: `${afterTrivial}\n${afterHard}`
          .split('\n')
          .filter((line) => /completion\(\)|POST \/v1\/chat/.test(line))
          .slice(-6)
          .map((line) => line.slice(0, 200)),
        screenshots: ['58-trivial-question.png', '58-hard-question.png'],
      });
      expect(routed).toBe(true);
    } finally {
      await api('DELETE', organizationPath, { token }).catch(() => undefined);
    }
  });
});
