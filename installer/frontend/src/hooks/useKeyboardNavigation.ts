import { useEffect } from 'react';

interface UseKeyboardNavigationProps {
  currentStep: number;
  totalSteps: number;
  onNext: () => void;
  onPrevious: () => void;
  enabled?: boolean;
}

export function useKeyboardNavigation({
  currentStep,
  totalSteps,
  onNext,
  onPrevious,
  enabled = true,
}: UseKeyboardNavigationProps) {
  useEffect(() => {
    if (!enabled) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't trigger when typing in inputs
      if (
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLSelectElement ||
        e.target instanceof HTMLTextAreaElement
      ) {
        return;
      }

      if (e.key === 'ArrowRight' && currentStep < totalSteps) {
        onNext();
      } else if (e.key === 'ArrowLeft' && currentStep > 1) {
        onPrevious();
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [currentStep, totalSteps, onNext, onPrevious, enabled]);
}
