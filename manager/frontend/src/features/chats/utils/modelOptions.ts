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
  tools?: boolean;
  reasoning?: boolean;
  needs_character?: boolean;
  capabilities?: string[] | null;
};

export function chatShowsAgent(chat: ChatHints, model?: ModelHints): boolean {
  return Boolean(chat.agent_enabled) || chat.tools === true || model?.tools === true;
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
