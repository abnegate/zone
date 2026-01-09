import { renderHook } from '@testing-library/react';
import { useKeyboardNavigation } from './useKeyboardNavigation';

describe('useKeyboardNavigation', () => {
  const onNext = jest.fn();
  const onPrevious = jest.fn();

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('calls onNext when ArrowRight is pressed and not at last step', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 1,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));

    expect(onNext).toHaveBeenCalled();
  });

  it('does not call onNext when at last step', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 3,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));

    expect(onNext).not.toHaveBeenCalled();
  });

  it('calls onPrevious when ArrowLeft is pressed and not at first step', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft' }));

    expect(onPrevious).toHaveBeenCalled();
  });

  it('does not call onPrevious when at first step', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 1,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft' }));

    expect(onPrevious).not.toHaveBeenCalled();
  });

  it('does not respond when disabled', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
        enabled: false,
      })
    );

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft' }));

    expect(onNext).not.toHaveBeenCalled();
    expect(onPrevious).not.toHaveBeenCalled();
  });

  it('ignores keyboard events when typing in input', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();

    const event = new KeyboardEvent('keydown', { key: 'ArrowRight' });
    Object.defineProperty(event, 'target', { value: input });
    document.dispatchEvent(event);

    expect(onNext).not.toHaveBeenCalled();

    document.body.removeChild(input);
  });

  it('ignores keyboard events when typing in select', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    const select = document.createElement('select');
    document.body.appendChild(select);
    select.focus();

    const event = new KeyboardEvent('keydown', { key: 'ArrowRight' });
    Object.defineProperty(event, 'target', { value: select });
    document.dispatchEvent(event);

    expect(onNext).not.toHaveBeenCalled();

    document.body.removeChild(select);
  });

  it('ignores keyboard events when typing in textarea', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    textarea.focus();

    const event = new KeyboardEvent('keydown', { key: 'ArrowLeft' });
    Object.defineProperty(event, 'target', { value: textarea });
    document.dispatchEvent(event);

    expect(onPrevious).not.toHaveBeenCalled();

    document.body.removeChild(textarea);
  });

  it('ignores other keys', () => {
    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp' }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));

    expect(onNext).not.toHaveBeenCalled();
    expect(onPrevious).not.toHaveBeenCalled();
  });

  it('cleans up event listener on unmount', () => {
    const removeEventListenerSpy = jest.spyOn(document, 'removeEventListener');

    const { unmount } = renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    unmount();

    expect(removeEventListenerSpy).toHaveBeenCalledWith('keydown', expect.any(Function));
    removeEventListenerSpy.mockRestore();
  });
});
