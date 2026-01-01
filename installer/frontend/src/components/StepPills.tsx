import React from 'react';
import { STEPS } from '../types';

interface StepPillsProps {
  currentStep: number;
  onStepClick: (step: number) => void;
}

export function StepPills({ currentStep, onStepClick }: StepPillsProps) {
  return (
    <nav className="steps-nav" aria-label="Installation steps">
      {STEPS.map(step => {
        const isActive = step.number === currentStep;
        const isCompleted = step.number < currentStep;

        let className = 'step-pill';
        if (isActive) className += ' active';
        if (isCompleted) className += ' completed';

        return (
          <button
            key={step.id}
            className={className}
            onClick={() => onStepClick(step.number)}
            aria-current={isActive ? 'step' : undefined}
            data-step={step.number}
          >
            {step.label}
          </button>
        );
      })}
    </nav>
  );
}
