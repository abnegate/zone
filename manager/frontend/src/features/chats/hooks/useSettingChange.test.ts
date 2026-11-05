import { describe, expect, it, mock } from 'bun:test';
import { act, renderHook } from '@testing-library/react';
import { writeSetting } from '../utils/panel';
import { useSettingChange } from './useSettingChange';

describe('useSettingChange', () => {
  it('reads the setting once it starts listening, so a change made before then is not missed', () => {
    const onChange = mock();

    renderHook(() => useSettingChange('zone:chat-panel:user-1:order', onChange));

    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('follows its own key in this tab, and any key cleared in another, but not other keys', () => {
    const onChange = mock();
    renderHook(() => useSettingChange('zone:chat-panel:user-1:order', onChange));
    onChange.mockClear();

    act(() => {
      writeSetting('zone:chat-panel:user-1:order', '[]');
      writeSetting('zone:chat-panel:user-1:width', '300');
      window.dispatchEvent(new StorageEvent('storage', { key: 'zone:chat-panel:user-1:order' }));
      window.dispatchEvent(new StorageEvent('storage', { key: 'zone:chat-panel:user-2:order' }));
      window.dispatchEvent(new StorageEvent('storage', { key: null }));
    });

    expect(onChange).toHaveBeenCalledTimes(3);
  });
});
