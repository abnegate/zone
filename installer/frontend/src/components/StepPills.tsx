import { Button, cn } from '@zone/ui';
import { STEPS } from '../types';

interface StepPillsProps {
  currentStep: number;
  onStepClick: (step: number) => void;
}

export function StepPills({ currentStep, onStepClick }: StepPillsProps) {
  return (
    <nav className="space-y-1.5" aria-label="Installation steps">
      {STEPS.map((step) => {
        const isActive = step.number === currentStep;
        const isCompleted = step.number < currentStep;

        return (
          <Button
            key={step.id}
            type="button"
            variant="ghost"
            onClick={() => onStepClick(step.number)}
            aria-current={isActive ? 'step' : undefined}
            className={cn(
              'h-auto w-full items-start justify-start gap-3 rounded-lg px-3 py-2 text-left whitespace-normal',
              isActive && 'bg-accent text-accent-foreground'
            )}
            data-step={step.number}
          >
            <span
              className={cn(
                'flex h-6 w-6 shrink-0 items-center justify-center rounded-full border text-[11px] font-medium',
                isCompleted && 'border-primary bg-primary text-primary-foreground',
                isActive && !isCompleted && 'border-primary/40 bg-primary/10 text-primary',
                !isActive && !isCompleted && 'border-border bg-background text-muted-foreground'
              )}
            >
              {isCompleted ? (
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="3"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  className="h-3 w-3"
                  aria-label="Step completed"
                  role="img"
                >
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              ) : (
                step.number
              )}
            </span>
            <span className="min-w-0 flex-1 flex flex-col items-start text-left">
              <span
                className={cn(
                  'text-sm font-medium leading-tight',
                  isActive || isCompleted ? 'text-foreground' : 'text-muted-foreground'
                )}
              >
                {step.label}
              </span>
              <span className="break-words text-xs leading-snug text-muted-foreground">
                {step.sidebarDescription}
              </span>
            </span>
          </Button>
        );
      })}
    </nav>
  );
}
