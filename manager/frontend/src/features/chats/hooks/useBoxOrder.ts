import { useCallback, useState } from 'react';
import { mergeOrder, moveBox, type PanelBoxId, parseOrder } from '../utils/boxes';
import { panelKey, readSetting, writeSetting } from '../utils/panel';
import { useSettingChange } from './useSettingChange';

export interface BoxOrder {
  order: PanelBoxId[];
  move: (shown: readonly PanelBoxId[], id: PanelBoxId, index: number) => void;
}

function readOrder(key: string): PanelBoxId[] {
  return mergeOrder(parseOrder(readSetting(key)));
}

/** The order a viewer keeps the chat panel's boxes in, remembered for them alone. */
export function useBoxOrder(userId: string | null): BoxOrder {
  const key = panelKey(userId, 'order');
  const [stored, setStored] = useState(() => ({ key, order: readOrder(key) }));
  const order = stored.key === key ? stored.order : readOrder(key);
  if (stored.key !== key) setStored({ key, order });

  useSettingChange(key, () => setStored({ key, order: readOrder(key) }));

  const move = useCallback(
    (shown: readonly PanelBoxId[], id: PanelBoxId, index: number) => {
      const next = moveBox(order, shown, id, index);
      setStored({ key, order: next });
      writeSetting(key, JSON.stringify(next));
    },
    [key, order]
  );

  return { order, move };
}
