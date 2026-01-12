import { fireEvent, renderHook } from '@testing-library/react';
import { useKeyboardNavigation } from './useKeyboardNavigation';

describe('useKeyboardNavigation', () => {
  const onNext = jest.fn();
  const onPrevious = jest.fn();

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('calls onNext when ArrowRight is pressed and not at last step', () => {
    const addEventListenerSpy = jest.spyOn(document, 'addEventListener');

    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 1,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    const handler = addEventListenerSpy.mock.calls.find(([eventName]) => eventName === 'keydown')?.[1];
    if (typeof handler === 'function') {
      handler(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
    }

    expect(onNext).toHaveBeenCalled();
    addEventListenerSpy.mockRestore();
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

    fireEvent.keyDown(document.body, { key: 'ArrowRight' });

    expect(onNext).not.toHaveBeenCalled();
  });

  it('calls onPrevious when ArrowLeft is pressed and not at first step', () => {
    const addEventListenerSpy = jest.spyOn(document, 'addEventListener');

    renderHook(() =>
      useKeyboardNavigation({
        currentStep: 2,
        totalSteps: 3,
        onNext,
        onPrevious,
      })
    );

    const handler = addEventListenerSpy.mock.calls.find(([eventName]) => eventName === 'keydown')?.[1];
    if (typeof handler === 'function') {
      handler(new KeyboardEvent('keydown', { key: 'ArrowLeft' }));
    }

    expect(onPrevious).toHaveBeenCalled();
    addEventListenerSpy.mockRestore();
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

    fireEvent.keyDown(document.body, { key: 'ArrowLeft' });

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

    fireEvent.keyDown(document.body, { key: 'ArrowRight' });
    fireEvent.keyDown(document.body, { key: 'ArrowLeft' });

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

    fireEvent.keyDown(input, { key: 'ArrowRight' });

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

    fireEvent.keyDown(select, { key: 'ArrowRight' });

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

    fireEvent.keyDown(textarea, { key: 'ArrowLeft' });

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

    fireEvent.keyDown(document.body, { key: 'Enter' });
    fireEvent.keyDown(document.body, { key: 'Escape' });
    fireEvent.keyDown(document.body, { key: 'ArrowUp' });
    fireEvent.keyDown(document.body, { key: 'ArrowDown' });

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
