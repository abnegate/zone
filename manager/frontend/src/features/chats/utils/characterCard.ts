import type { ChatCharacter } from '../types';

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const MAX_PNG_BYTES = 8 * 1024 * 1024;
const MAX_CHUNK = 2 * 1024 * 1024;

function nonempty(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
}

function stopsFrom(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === 'string' && item.trim() !== '');
  }
  if (typeof value === 'string' && value.trim()) {
    return [value.trim()];
  }
  return [];
}

function interpolate(value: string, name: string): string {
  return value
    .replaceAll('{{char}}', name)
    .replaceAll('{{Char}}', name)
    .replaceAll('{{user}}', 'User')
    .replaceAll('{{User}}', 'User');
}

export function characterSystemPrompt(card: ChatCharacter): string {
  const name = card.name.trim();
  const sections: string[] = [];
  if (card.system_prompt) sections.push(interpolate(card.system_prompt, name));
  if (card.description) sections.push(interpolate(card.description, name));
  if (card.personality) sections.push(`Personality:\n${interpolate(card.personality, name)}`);
  if (card.scenario) sections.push(`Scenario:\n${interpolate(card.scenario, name)}`);
  if (card.mes_example) sections.push(`Example dialogue:\n${interpolate(card.mes_example, name)}`);
  if (card.post_history_instructions) {
    sections.push(interpolate(card.post_history_instructions, name));
  }
  if (sections.length === 0) {
    return `You are ${name}. Stay in character. Write ${name}'s next reply.`;
  }
  if (!card.system_prompt) {
    sections.push(`Stay in character as ${name}. Write ${name}'s next reply.`);
  }
  return sections.join('\n\n');
}

export function parseCharacterJson(value: unknown, sourceName?: string): ChatCharacter | null {
  if (!value || typeof value !== 'object') return null;
  const root = value as Record<string, unknown>;
  const spec = typeof root.spec === 'string' ? root.spec.toLowerCase() : '';
  const data =
    spec.includes('chara_card') && root.data && typeof root.data === 'object'
      ? (root.data as Record<string, unknown>)
      : root;
  const name = nonempty(data.name) ?? nonempty(root.name);
  if (!name) return null;
  const extensions =
    data.extensions && typeof data.extensions === 'object'
      ? (data.extensions as Record<string, unknown>)
      : {};
  return {
    name,
    description: nonempty(data.description),
    personality: nonempty(data.personality),
    scenario: nonempty(data.scenario),
    first_mes: nonempty(data.first_mes),
    mes_example: nonempty(data.mes_example),
    system_prompt: nonempty(data.system_prompt),
    post_history_instructions: nonempty(data.post_history_instructions),
    stop_sequences: stopsFrom(data.stop_sequences ?? extensions.stop_sequences),
    source_name: sourceName,
  };
}

export function looksLikeCharacterCard(
  card: ChatCharacter,
  sourceName?: string,
  raw?: unknown
): boolean {
  if (sourceName && /char(acter)?|card|chara/i.test(sourceName)) return true;
  if (raw && typeof raw === 'object' && raw && 'spec' in raw) {
    const spec = String((raw as { spec?: unknown }).spec ?? '');
    if (spec.toLowerCase().includes('chara_card')) return true;
  }
  return Boolean(
    card.first_mes || card.personality || card.scenario || card.system_prompt || card.mes_example
  );
}

function pngKeywordText(bytes: Uint8Array): string | null {
  if (bytes.length > MAX_PNG_BYTES) return null;
  for (let i = 0; i < PNG_SIGNATURE.length; i += 1) {
    if (bytes[i] !== PNG_SIGNATURE[i]) return null;
  }
  let offset = PNG_SIGNATURE.length;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const decoder = new TextDecoder();
  while (offset + 12 <= bytes.length) {
    const length = view.getUint32(offset);
    if (length > MAX_CHUNK) return null;
    const type = decoder.decode(bytes.subarray(offset + 4, offset + 8));
    const dataStart = offset + 8;
    const dataEnd = dataStart + length;
    if (dataEnd + 4 > bytes.length) return null;
    if (type === 'tEXt' || type === 'iTXt') {
      const data = bytes.subarray(dataStart, dataEnd);
      const split = data.indexOf(0);
      if (split > 0) {
        const keyword = decoder.decode(data.subarray(0, split));
        if (keyword === 'chara' || keyword === 'ccv3') {
          const rest = data.subarray(split + 1);
          if (type === 'iTXt') {
            if (rest.length < 3 || rest[0] !== 0) return null;
            let cursor = 2;
            const langEnd = rest.indexOf(0, cursor);
            if (langEnd < 0) return null;
            cursor = langEnd + 1;
            const translatedEnd = rest.indexOf(0, cursor);
            if (translatedEnd < 0) return null;
            return decoder.decode(rest.subarray(translatedEnd + 1));
          }
          return decoder.decode(rest);
        }
      }
    }
    if (type === 'IEND') break;
    offset = dataEnd + 4;
  }
  return null;
}

export function parseCharacterPng(bytes: Uint8Array, sourceName?: string): ChatCharacter | null {
  const encoded = pngKeywordText(bytes);
  if (!encoded) return null;
  try {
    return parseCharacterJson(JSON.parse(atob(encoded.trim())), sourceName);
  } catch {
    return null;
  }
}

export function parseCharacterText(text: string, sourceName?: string): ChatCharacter | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith('{')) {
    try {
      return parseCharacterJson(JSON.parse(trimmed), sourceName);
    } catch {
      return null;
    }
  }
  return {
    name: 'Character',
    system_prompt: trimmed,
    source_name: sourceName,
  };
}

export async function parseCharacterFile(file: File): Promise<ChatCharacter | null> {
  if (file.type === 'image/png' || file.name.toLowerCase().endsWith('.png')) {
    const bytes = new Uint8Array(await file.arrayBuffer());
    return parseCharacterPng(bytes, file.name);
  }
  if (
    file.type.startsWith('text/') ||
    file.type === 'application/json' ||
    /\.(json|txt|md)$/i.test(file.name)
  ) {
    const card = parseCharacterText(await file.text(), file.name);
    if (!card) return null;
    if (looksLikeCharacterCard(card, file.name)) return card;
    return null;
  }
  return null;
}
