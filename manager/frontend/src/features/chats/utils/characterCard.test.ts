import { describe, expect, it } from 'bun:test';
import {
  characterSystemPrompt,
  looksLikeCharacterCard,
  parseCharacterJson,
  parseCharacterPng,
  parseCharacterText,
} from './characterCard';

function pngWithText(keyword: string, text: string): Uint8Array {
  const encoder = new TextEncoder();
  const data = new Uint8Array(keyword.length + 1 + text.length);
  data.set(encoder.encode(keyword), 0);
  data[keyword.length] = 0;
  data.set(encoder.encode(text), keyword.length + 1);
  const png = new Uint8Array(8 + 12 + data.length + 12);
  png.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 0);
  const view = new DataView(png.buffer);
  view.setUint32(8, data.length);
  png.set(encoder.encode('tEXt'), 12);
  png.set(data, 16);
  view.setUint32(16 + data.length + 4, 0);
  png.set(encoder.encode('IEND'), 16 + data.length + 8);
  return png;
}

describe('character cards', () => {
  it('parses a V2 card and interpolates the persona', () => {
    const card = parseCharacterJson({
      spec: 'chara_card_v2',
      data: {
        name: 'Noromaid',
        description: '{{char}} keeps the bar.',
        personality: 'Warm',
        first_mes: 'Welcome in.',
        system_prompt: 'Stay in character as {{char}}.',
      },
    });
    expect(card?.name).toBe('Noromaid');
    expect(card?.first_mes).toBe('Welcome in.');
    const prompt = characterSystemPrompt(card!);
    expect(prompt).toContain('Stay in character as Noromaid.');
    expect(prompt).toContain('Noromaid keeps the bar.');
    expect(prompt).not.toContain('{{char}}');
  });

  it('treats plain text as a system prompt', () => {
    const card = parseCharacterText('You are a tired ship cook.');
    expect(card).toEqual({
      name: 'Character',
      system_prompt: 'You are a tired ship cook.',
      source_name: undefined,
    });
  });

  it('reads a PNG chara chunk', () => {
    const payload = btoa(
      JSON.stringify({ spec: 'chara_card_v2', data: { name: 'Pixel', description: 'Drawn' } })
    );
    const card = parseCharacterPng(pngWithText('chara', payload), 'pixel.png');
    expect(card?.name).toBe('Pixel');
    expect(card?.source_name).toBe('pixel.png');
  });

  it('does not treat ordinary JSON attachments as cards', () => {
    const card = parseCharacterJson({ name: 'package', version: '1.0.0' }, 'package.json');
    expect(card?.name).toBe('package');
    expect(looksLikeCharacterCard(card!, 'package.json', { name: 'package' })).toBe(false);
  });
});
