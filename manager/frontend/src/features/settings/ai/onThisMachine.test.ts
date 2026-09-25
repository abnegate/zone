import { describe, expect, it } from 'bun:test';
import { onThisMachine } from './onThisMachine';

describe('onThisMachine', () => {
  it('takes every loopback name for this machine', () => {
    for (const hostname of [
      'localhost',
      'LocalHost',
      'manager.localhost',
      'zone.manager.localhost',
      '127.0.0.1',
      '[::1]',
    ]) {
      expect(onThisMachine(hostname)).toBe(true);
    }
  });

  it('takes any other name for another machine', () => {
    for (const hostname of [
      'zone.example.com',
      'localhost.attacker.example',
      'attacker-localhost',
      '.localhost',
      'manager..localhost',
      '10.0.0.5',
      '127.0.0.2',
      '0.0.0.0',
      '',
    ]) {
      expect(onThisMachine(hostname)).toBe(false);
    }
  });
});
