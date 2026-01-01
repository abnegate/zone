import React from 'react';
import { STEPS } from '../types';

interface ProgressBarProps {
  currentStep: number;
}

export function ProgressBar({ currentStep }: ProgressBarProps) {
  const totalSteps = STEPS.length;
  const percentage = Math.round((currentStep / totalSteps) * 100);

  return (
    <div className="progress-container">
      <div className="progress-header">
        <span>Step {currentStep} of {totalSteps}</span>
        <span>{percentage}%</span>
      </div>
      <div className="progress-bar-track">
        <div
          className="progress-bar-fill"
          style={{ width: `${percentage}%` }}
        />
      </div>
    </div>
  );
}
