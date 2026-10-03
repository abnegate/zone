import { formatContextLength } from '../../models/utils/formatters';

export const AUTO_MODEL = 'auto';

export const DEFAULT_CONTEXT_TOKENS = 32_768;

export const CONTEXT_TOKEN_STEPS = [
  2_048, 4_096, 8_192, 16_384, 32_768, 65_536, 131_072, 262_144, 524_288, 1_048_576,
] as const;

type LocalContextModel = {
  size?: number;
  details?: { context_length?: number | null } | null;
};

export function offersLocalContext(model?: LocalContextModel): boolean {
  if (!model) return false;
  if ((model.details?.context_length ?? 0) > 0) return true;
  return (model.size ?? 0) > 0;
}

export function contextTokenOptions(
  native?: number | null
): Array<{ value: string; label: string }> {
  const ceiling =
    native && native > 0 ? native : CONTEXT_TOKEN_STEPS[CONTEXT_TOKEN_STEPS.length - 1];
  const values = CONTEXT_TOKEN_STEPS.filter((step) => step <= ceiling);
  const tokens =
    native && native > 0 && !values.some((step) => step === native)
      ? [...values, native].sort((left, right) => left - right)
      : values;
  return tokens.map((value) => ({
    value: String(value),
    label: `${formatContextLength(value)} tokens`,
  }));
}

export function defaultContextTokens(native?: number | null): number {
  if (native && native > 0) return Math.min(DEFAULT_CONTEXT_TOKENS, native);
  return DEFAULT_CONTEXT_TOKENS;
}

export function selectedContextTokens(
  chosen: number | null | undefined,
  native?: number | null
): number {
  if (chosen && chosen > 0) return chosen;
  if (native && native > 0) return native;
  return DEFAULT_CONTEXT_TOKENS;
}

export function modelLabel(name: string): string {
  return name === AUTO_MODEL ? 'Automatic' : name;
}

export function sameModelName(left: string, right: string): boolean {
  if (left === right) return true;
  const strip = (name: string) => name.replace(/:latest$/i, '');
  return strip(left) === strip(right);
}

export function findInstalledModel<T extends { name: string }>(
  models: T[],
  name: string
): T | undefined {
  return models.find((model) => sameModelName(model.name, name));
}

type ChatHints = {
  agent_enabled?: boolean;
  character?: unknown;
  tools?: boolean | null;
  reasoning?: boolean | null;
  needs_character?: boolean | null;
};

type ModelHints = {
  completion?: boolean;
  tools?: boolean;
  reasoning?: boolean;
  needs_character?: boolean;
  capabilities?: string[] | null;
};

export function offersAgent(model?: ModelHints): boolean {
  return model !== undefined && model.completion !== false && model.tools !== false;
}

export function chatShowsAgent(chat: ChatHints, model?: ModelHints): boolean {
  return (
    Boolean(chat.agent_enabled) ||
    chat.tools === true ||
    (chat.tools !== false && offersAgent(model))
  );
}

export function chatShowsCharacter(chat: ChatHints, model?: ModelHints): boolean {
  return (
    Boolean(chat.character) || chat.needs_character === true || model?.needs_character === true
  );
}

export function chatShowsReasoning(chat: ChatHints, model?: ModelHints): boolean {
  return (
    chat.reasoning === true ||
    model?.reasoning === true ||
    Boolean(model?.capabilities?.includes('reasoning'))
  );
}
