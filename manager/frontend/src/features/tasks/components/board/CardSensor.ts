import { type DraggableNode, PointerSensor, type PointerSensorOptions } from '@dnd-kit/core';
import type { PointerEvent, SyntheticEvent } from 'react';

const TOUCH = 'touch';

interface Activation {
  active: DraggableNode;
}

function grabs(
  { nativeEvent: event }: PointerEvent,
  { onActivation }: PointerSensorOptions,
  activation?: Activation
): boolean {
  if (!event.isPrimary || event.button !== 0) return false;
  if (event.pointerType === TOUCH) {
    const handle = activation?.active.activatorNode.current;
    if (!(event.target instanceof Node) || !handle?.contains(event.target)) return false;
  }
  onActivation?.({ event });
  return true;
}

export class CardSensor extends PointerSensor {
  static activators = [{ eventName: 'onPointerDown' as const, handler: grabs }];

  static shield(event: SyntheticEvent): void {
    event.stopPropagation();
  }
}
