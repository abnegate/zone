import React, { forwardRef, useEffect, useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '../../lib/utils';
import { Button } from '../Button';

export type WizardSize = 'sm' | 'md' | 'lg' | 'xl';

type StepState = 'completed' | 'current' | 'upcoming';

export interface WizardStep {
  id: string;
  title: string;
  description?: string;
  icon?: React.ReactNode;
}

export interface WizardProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'> {
  isOpen: boolean;
  onClose?: () => void;
  title: string;
  subtitle?: string;
  steps: WizardStep[];
  currentStep: number;
  onStepChange?: (step: number) => void;
  onComplete?: () => void;
  onCancel?: () => void;
  completeLabel?: string;
  nextLabel?: string;
  previousLabel?: string;
  cancelLabel?: string;
  loading?: boolean;
  canProceed?: boolean;
  showStepNumbers?: boolean;
  allowStepClick?: boolean;
  size?: WizardSize | null;
}

const STEP_TRANSITION_MS = 150;

const Wizard = forwardRef<HTMLDivElement, WizardProps>(
  (
    {
      isOpen,
      onClose,
      title,
      subtitle,
      steps,
      currentStep,
      onStepChange,
      onComplete,
      onCancel,
      completeLabel = 'Complete',
      nextLabel = 'Next',
      previousLabel = 'Previous',
      cancelLabel = 'Cancel',
      loading = false,
      canProceed = true,
      showStepNumbers = true,
      allowStepClick = false,
      size,
      children,
      className,
      ...props
    },
    ref
  ) => {
    const [animatingStep, setAnimatingStep] = useState<'next' | 'prev' | null>(null);

    useEffect(() => {
      if (!isOpen) return undefined;

      const handleEscape = (e: KeyboardEvent) => {
        if (e.key === 'Escape' && onClose) {
          onClose();
        }
      };

      const scrollbarWidth = window.innerWidth - document.documentElement.clientWidth;
      const previousOverflow = document.body.style.overflow;
      const previousPaddingRight = document.body.style.paddingRight;

      document.addEventListener('keydown', handleEscape);
      document.body.style.overflow = 'hidden';
      if (scrollbarWidth > 0) {
        document.body.style.paddingRight = `${scrollbarWidth}px`;
      }

      return () => {
        document.removeEventListener('keydown', handleEscape);
        document.body.style.overflow = previousOverflow;
        document.body.style.paddingRight = previousPaddingRight;
      };
    }, [isOpen, onClose]);

    const transitionTo = useCallback(
      (step: number, direction: 'next' | 'prev') => {
        setAnimatingStep(direction);
        setTimeout(() => {
          onStepChange?.(step);
          setAnimatingStep(null);
        }, STEP_TRANSITION_MS);
      },
      [onStepChange]
    );

    const handleNext = useCallback(() => {
      if (currentStep < steps.length - 1 && canProceed && !loading) {
        transitionTo(currentStep + 1, 'next');
      }
    }, [currentStep, steps.length, canProceed, loading, transitionTo]);

    const handlePrevious = useCallback(() => {
      if (currentStep > 0 && !loading) {
        transitionTo(currentStep - 1, 'prev');
      }
    }, [currentStep, loading, transitionTo]);

    const handleStepClick = useCallback(
      (stepIndex: number) => {
        if (!allowStepClick || loading) return;
        if (stepIndex < currentStep) {
          transitionTo(stepIndex, 'prev');
        } else if (stepIndex > currentStep && canProceed) {
          transitionTo(stepIndex, 'next');
        }
      },
      [allowStepClick, loading, currentStep, canProceed, transitionTo]
    );

    const handleComplete = useCallback(() => {
      if (canProceed && !loading) {
        onComplete?.();
      }
    }, [canProceed, loading, onComplete]);

    const handleCancel = useCallback(() => {
      if (!loading) {
        onCancel?.();
        onClose?.();
      }
    }, [loading, onCancel, onClose]);

    if (!isOpen) return null;

    const isLastStep = currentStep === steps.length - 1;
    const isFirstStep = currentStep === 0;

    const stateOf = (index: number): StepState => {
      if (index < currentStep) return 'completed';
      if (index === currentStep) return 'current';
      return 'upcoming';
    };

    const dialog = (
      <div className="ui-wizard-overlay">
        <button
          type="button"
          className="ui-wizard-dismiss"
          aria-label="Close wizard"
          tabIndex={-1}
          onClick={onClose}
        />
        <div
          ref={ref}
          className={cn('ui-wizard', `ui-wizard--${size ?? 'md'}`, className)}
          role="dialog"
          aria-modal="true"
          aria-labelledby="wizard-title"
          {...props}
        >
          <header className="ui-wizard-header">
            <div className="ui-wizard-heading">
              <h2 id="wizard-title">{title}</h2>
              {subtitle && <p className="ui-wizard-subtitle">{subtitle}</p>}
            </div>
            {onClose && (
              <button
                type="button"
                className="ui-wizard-close"
                onClick={onClose}
                aria-label="Close wizard"
                disabled={loading}
              >
                <svg
                  aria-hidden="true"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M18 6L6 18M6 6l12 12" />
                </svg>
              </button>
            )}
          </header>

          <nav className="ui-wizard-steps" aria-label="Wizard steps">
            <ol>
              {steps.map((step, index) => {
                const state = stateOf(index);
                const clickable =
                  allowStepClick &&
                  (state === 'completed' || (canProceed && index === currentStep + 1));

                return (
                  <li key={step.id}>
                    <button
                      type="button"
                      className="ui-wizard-step"
                      data-state={state}
                      data-clickable={clickable}
                      onClick={() => handleStepClick(index)}
                      disabled={!clickable || loading}
                      aria-current={state === 'current' ? 'step' : undefined}
                    >
                      <span className="ui-wizard-step-indicator">
                        {state === 'completed' ? (
                          <svg
                            aria-hidden="true"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="3"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          >
                            <polyline points="20 6 9 17 4 12" />
                          </svg>
                        ) : step.icon ? (
                          step.icon
                        ) : showStepNumbers ? (
                          index + 1
                        ) : null}
                      </span>
                      <span className="ui-wizard-step-copy">
                        <span className="ui-wizard-step-title">{step.title}</span>
                        {step.description && (
                          <span className="ui-wizard-step-description">{step.description}</span>
                        )}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ol>
          </nav>

          <div className="ui-wizard-content" data-animating={animatingStep ?? 'none'}>
            {children}
          </div>

          <footer className="ui-wizard-footer">
            <div>
              <Button variant="ghost" onClick={handleCancel} disabled={loading}>
                {cancelLabel}
              </Button>
            </div>
            <div className="ui-wizard-actions">
              {!isFirstStep && (
                <Button variant="secondary" onClick={handlePrevious} disabled={loading}>
                  {previousLabel}
                </Button>
              )}
              {isLastStep ? (
                <Button
                  variant="primary"
                  onClick={handleComplete}
                  disabled={!canProceed || loading}
                  loading={loading}
                >
                  {completeLabel}
                </Button>
              ) : (
                <Button variant="primary" onClick={handleNext} disabled={!canProceed || loading}>
                  {nextLabel}
                </Button>
              )}
            </div>
          </footer>
        </div>
      </div>
    );

    if (typeof document === 'undefined') {
      return dialog;
    }

    return createPortal(dialog, document.body);
  }
);

Wizard.displayName = 'Wizard';

export { Wizard };
