import React, { forwardRef, useEffect, useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';
import { Button } from '../Button';

const overlayVariants = cva([
  'fixed inset-0 z-50',
  'flex items-center justify-center',
  'bg-[var(--ui-overlay-medium)]',
  'backdrop-blur-sm',
]);

const wizardVariants = cva(
  [
    'relative flex flex-col',
    'bg-[var(--ui-bg-elevated)]',
    'border border-[var(--ui-border)]',
    'rounded-[var(--ui-radius-xl)]',
    'shadow-[var(--ui-shadow-xl)]',
    'max-h-[90vh] overflow-hidden',
  ],
  {
    variants: {
      size: {
        sm: 'w-full max-w-lg',
        md: 'w-full max-w-2xl',
        lg: 'w-full max-w-4xl',
        xl: 'w-full max-w-6xl',
      },
    },
    defaultVariants: {
      size: 'md',
    },
  }
);

const headerVariants = cva([
  'flex items-start justify-between gap-[var(--ui-space-4)]',
  'p-[var(--ui-space-6)]',
  'border-b border-[var(--ui-border)]',
]);

const titleVariants = cva([
  'text-[var(--ui-text-xl)] font-semibold',
  'text-[var(--ui-text-primary)]',
]);

const subtitleVariants = cva([
  'mt-[var(--ui-space-1)]',
  'text-[var(--ui-text-sm)]',
  'text-[var(--ui-text-muted)]',
]);

const closeButtonVariants = cva([
  'flex items-center justify-center',
  'w-8 h-8',
  'rounded-[var(--ui-radius-md)]',
  'text-[var(--ui-text-muted)]',
  'hover:bg-[var(--ui-bg-hover)] hover:text-[var(--ui-text-primary)]',
  'transition-colors duration-[var(--ui-duration-fast)]',
  'disabled:opacity-50 disabled:cursor-not-allowed',
]);

const stepsNavVariants = cva([
  'px-[var(--ui-space-6)] py-[var(--ui-space-4)]',
  'border-b border-[var(--ui-border)]',
  'bg-[var(--ui-bg-surface)]',
]);

const progressTrackVariants = cva([
  'h-1 w-full',
  'bg-[var(--ui-bg-muted)]',
  'rounded-full',
  'mb-[var(--ui-space-4)]',
  'overflow-hidden',
]);

const progressFillVariants = cva([
  'h-full',
  'bg-gradient-to-r from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]',
  'rounded-full',
  'transition-all duration-[var(--ui-duration-normal)] ease-out',
]);

const stepListVariants = cva([
  'flex items-center justify-between gap-[var(--ui-space-2)]',
  'list-none m-0 p-0',
]);

const stepItemVariants = cva(
  ['flex-1'],
  {
    variants: {
      state: {
        completed: '',
        current: '',
        upcoming: '',
      },
      clickable: {
        true: 'cursor-pointer',
        false: '',
      },
    },
    defaultVariants: {
      state: 'upcoming',
      clickable: false,
    },
  }
);

const stepButtonVariants = cva(
  [
    'flex items-center gap-[var(--ui-space-3)] w-full',
    'p-[var(--ui-space-2)]',
    'rounded-[var(--ui-radius-md)]',
    'transition-colors duration-[var(--ui-duration-fast)]',
    'disabled:cursor-not-allowed',
  ],
  {
    variants: {
      state: {
        completed: 'hover:bg-[var(--ui-bg-hover)]',
        current: 'bg-[var(--ui-accent-muted)]',
        upcoming: 'opacity-50',
      },
      clickable: {
        true: 'hover:bg-[var(--ui-bg-hover)]',
        false: '',
      },
    },
    defaultVariants: {
      state: 'upcoming',
      clickable: false,
    },
  }
);

const stepIndicatorVariants = cva(
  [
    'flex items-center justify-center shrink-0',
    'w-8 h-8',
    'rounded-full',
    'text-[var(--ui-text-sm)] font-medium',
    'transition-colors duration-[var(--ui-duration-fast)]',
  ],
  {
    variants: {
      state: {
        completed: 'bg-[var(--ui-accent-500)] text-white',
        current: 'bg-[var(--ui-accent-500)] text-white',
        upcoming: 'bg-[var(--ui-bg-muted)] text-[var(--ui-text-muted)]',
      },
    },
    defaultVariants: {
      state: 'upcoming',
    },
  }
);

const stepTitleVariants = cva(
  ['text-[var(--ui-text-sm)] font-medium'],
  {
    variants: {
      state: {
        completed: 'text-[var(--ui-text-primary)]',
        current: 'text-[var(--ui-text-primary)]',
        upcoming: 'text-[var(--ui-text-muted)]',
      },
    },
    defaultVariants: {
      state: 'upcoming',
    },
  }
);

const stepDescriptionVariants = cva([
  'text-[var(--ui-text-xs)]',
  'text-[var(--ui-text-muted)]',
]);

const contentVariants = cva(
  [
    'flex-1 overflow-auto',
    'p-[var(--ui-space-6)]',
    'transition-all duration-150 ease-out',
  ],
  {
    variants: {
      animating: {
        next: 'opacity-0 -translate-x-4',
        prev: 'opacity-0 translate-x-4',
        none: 'opacity-100 translate-x-0',
      },
    },
    defaultVariants: {
      animating: 'none',
    },
  }
);

const footerVariants = cva([
  'flex items-center justify-between',
  'p-[var(--ui-space-6)]',
  'border-t border-[var(--ui-border)]',
  'bg-[var(--ui-bg-surface)]',
]);

export interface WizardStep {
  id: string;
  title: string;
  description?: string;
  icon?: React.ReactNode;
}

export interface WizardProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'>,
    VariantProps<typeof wizardVariants> {
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
}

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

    const handleNext = useCallback(() => {
      if (currentStep < steps.length - 1 && canProceed && !loading) {
        setAnimatingStep('next');
        setTimeout(() => {
          onStepChange?.(currentStep + 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, steps.length, canProceed, loading, onStepChange]);

    const handlePrevious = useCallback(() => {
      if (currentStep > 0 && !loading) {
        setAnimatingStep('prev');
        setTimeout(() => {
          onStepChange?.(currentStep - 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, loading, onStepChange]);

    const handleStepClick = useCallback(
      (stepIndex: number) => {
        if (!allowStepClick || loading) return;
        if (stepIndex < currentStep) {
          setAnimatingStep('prev');
          setTimeout(() => {
            onStepChange?.(stepIndex);
            setAnimatingStep(null);
          }, 150);
        } else if (stepIndex > currentStep && canProceed) {
          setAnimatingStep('next');
          setTimeout(() => {
            onStepChange?.(stepIndex);
            setAnimatingStep(null);
          }, 150);
        }
      },
      [allowStepClick, loading, currentStep, canProceed, onStepChange]
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
    const progressPercent = ((currentStep + 1) / steps.length) * 100;

    const getStepState = (index: number): 'completed' | 'current' | 'upcoming' => {
      if (index < currentStep) return 'completed';
      if (index === currentStep) return 'current';
      return 'upcoming';
    };

    const dialog = (
      <div className={cn('ui-wizard-overlay', overlayVariants())} onClick={onClose}>
        <div
          ref={ref}
          className={cn('ui-wizard', `ui-wizard--${size ?? 'md'}`, wizardVariants({ size, className }))}
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-labelledby="wizard-title"
          {...props}
        >
          {/* Header */}
          <header className={cn('ui-wizard-header', headerVariants())}>
            <div>
              <h2 id="wizard-title" className={cn(titleVariants())}>
                {title}
              </h2>
              {subtitle && <p className={cn('ui-wizard-subtitle', subtitleVariants())}>{subtitle}</p>}
            </div>
            {onClose && (
              <button
                type="button"
                className={cn('ui-wizard-close', closeButtonVariants())}
                onClick={onClose}
                aria-label="Close wizard"
                disabled={loading}
              >
                <svg
                  className="w-5 h-5"
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

          {/* Step Indicator */}
          <nav className={cn('ui-wizard-steps', stepsNavVariants())} aria-label="Wizard steps">
            <div className={cn('ui-wizard-progress', progressTrackVariants())}>
              <div
                className={cn(progressFillVariants())}
                style={{ width: `${progressPercent}%` }}
              />
            </div>
            <ol className={cn(stepListVariants())}>
              {steps.map((step, index) => {
                const state = getStepState(index);
                const isClickable = allowStepClick && (state === 'completed' || (canProceed && index === currentStep + 1));

                return (
                  <li
                    key={step.id}
                    className={cn(stepItemVariants({ state, clickable: isClickable }))}
                  >
                    <button
                      type="button"
                      className={cn(stepButtonVariants({ state, clickable: isClickable }))}
                      onClick={() => handleStepClick(index)}
                      disabled={!isClickable || loading}
                      aria-current={state === 'current' ? 'step' : undefined}
                    >
                      <span className={cn(stepIndicatorVariants({ state }))}>
                        {state === 'completed' ? (
                          <svg
                            className="w-4 h-4"
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
                        ) : (
                          <span className="w-2 h-2 rounded-full bg-current" />
                        )}
                      </span>
                      <span className="ui-wizard-step-copy flex flex-col items-start">
                        <span className={cn('ui-wizard-step-title', stepTitleVariants({ state }))}>{step.title}</span>
                        {step.description && (
                          <span className={cn('ui-wizard-step-description', stepDescriptionVariants())}>{step.description}</span>
                        )}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ol>
          </nav>

          {/* Content */}
          <div
            className={cn('ui-wizard-content', contentVariants({ animating: animatingStep || 'none' }))}
          >
            {children}
          </div>

          {/* Footer */}
          <footer className={cn('ui-wizard-footer', footerVariants())}>
            <div>
              <Button
                variant="ghost"
                onClick={handleCancel}
                disabled={loading}
              >
                {cancelLabel}
              </Button>
            </div>
            <div className="flex items-center gap-[var(--ui-space-3)]">
              {!isFirstStep && (
                <Button
                  variant="secondary"
                  onClick={handlePrevious}
                  disabled={loading}
                >
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
                <Button
                  variant="primary"
                  onClick={handleNext}
                  disabled={!canProceed || loading}
                >
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

export { Wizard, wizardVariants };
