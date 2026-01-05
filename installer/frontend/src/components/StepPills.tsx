import { STEPS } from '../types';

interface StepPillsProps {
  currentStep: number;
  onStepClick: (step: number) => void;
}

const STEP_DESCRIPTIONS: Record<string, string> = {
  domain: 'Configure your domain settings',
  security: 'Set up authentication and keys',
  models: 'Choose your AI models',
  interface: 'Customize the web interface',
  search: 'Configure search settings',
  vpn: 'Set up VPN connection',
  advanced: 'Fine-tune advanced options',
};

export function StepPills({ currentStep, onStepClick }: StepPillsProps) {
  return (
    <nav className="vertical-stepper" aria-label="Installation steps">
      {STEPS.map((step, index) => {
        const isActive = step.number === currentStep;
        const isCompleted = step.number < currentStep;
        const isLast = index === STEPS.length - 1;

        let className = 'stepper-item';
        if (isActive) className += ' active';
        if (isCompleted) className += ' completed';

        return (
          <div key={step.id} className={className}>
            <button
              className="stepper-button"
              onClick={() => onStepClick(step.number)}
              aria-current={isActive ? 'step' : undefined}
            >
              <span className="stepper-indicator">
                {isCompleted ? (
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.5"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                ) : (
                  step.number
                )}
              </span>
              <span className="stepper-content">
                <span className="stepper-label">{step.label}</span>
                <span className="stepper-description">{STEP_DESCRIPTIONS[step.id]}</span>
              </span>
            </button>
            {!isLast && <div className="stepper-connector" />}
          </div>
        );
      })}
    </nav>
  );
}
