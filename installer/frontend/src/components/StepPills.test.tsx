import { fireEvent, render, screen } from '@testing-library/react';
import { STEPS } from '../types';
import { StepPills } from './StepPills';

describe('StepPills', () => {
  const onStepClick = jest.fn();

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders all steps', () => {
    render(<StepPills currentStep={1} onStepClick={onStepClick} />);

    STEPS.forEach((step) => {
      expect(screen.getByText(step.label)).toBeInTheDocument();
    });
  });

  it('marks current step as active', () => {
    render(<StepPills currentStep={3} onStepClick={onStepClick} />);

    const activeStep = screen.getByRole('button', { current: 'step' });
    expect(activeStep).toBeInTheDocument();
  });

  it('displays step numbers for non-completed steps', () => {
    render(<StepPills currentStep={4} onStepClick={onStepClick} />);

    // Steps 4, 5, 6, 7 should show numbers
    expect(screen.getByText('4')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
  });

  it('displays checkmarks for completed steps', () => {
    render(<StepPills currentStep={4} onStepClick={onStepClick} />);

    // Steps 1, 2, 3 should have checkmarks (SVG elements)
    const svgs = document.querySelectorAll('svg');
    expect(svgs.length).toBe(3);
  });

  it('applies correct classes to steps', () => {
    render(<StepPills currentStep={3} onStepClick={onStepClick} />);

    const stepItems = document.querySelectorAll('.stepper-item');
    expect(stepItems[0]).toHaveClass('completed');
    expect(stepItems[1]).toHaveClass('completed');
    expect(stepItems[2]).toHaveClass('active');
    expect(stepItems[3]).not.toHaveClass('completed');
    expect(stepItems[3]).not.toHaveClass('active');
  });

  it('calls onStepClick when step is clicked', () => {
    render(<StepPills currentStep={1} onStepClick={onStepClick} />);

    fireEvent.click(screen.getByText(STEPS[2].label));

    expect(onStepClick).toHaveBeenCalledWith(STEPS[2].number);
  });

  it('renders navigation landmark', () => {
    render(<StepPills currentStep={1} onStepClick={onStepClick} />);

    expect(screen.getByRole('navigation', { name: /installation steps/i })).toBeInTheDocument();
  });

  it('renders step descriptions', () => {
    render(<StepPills currentStep={1} onStepClick={onStepClick} />);

    expect(screen.getByText('Configure your domain settings')).toBeInTheDocument();
    expect(screen.getByText('Set up authentication and keys')).toBeInTheDocument();
    expect(screen.getByText('Choose your AI models')).toBeInTheDocument();
    expect(screen.getByText('Customize the web interface')).toBeInTheDocument();
    expect(screen.getByText('Configure search settings')).toBeInTheDocument();
    expect(screen.getByText('Set up VPN connection')).toBeInTheDocument();
    expect(screen.getByText('Fine-tune advanced options')).toBeInTheDocument();
  });

  it('does not render connector after last step', () => {
    render(<StepPills currentStep={1} onStepClick={onStepClick} />);

    const connectors = document.querySelectorAll('.stepper-connector');
    // Should have connectors for all steps except the last one
    expect(connectors.length).toBe(STEPS.length - 1);
  });

  it('handles first step correctly', () => {
    render(<StepPills currentStep={1} onStepClick={onStepClick} />);

    const stepItems = document.querySelectorAll('.stepper-item');
    expect(stepItems[0]).toHaveClass('active');
    expect(stepItems[0]).not.toHaveClass('completed');
  });

  it('handles last step correctly', () => {
    render(<StepPills currentStep={STEPS.length} onStepClick={onStepClick} />);

    const stepItems = document.querySelectorAll('.stepper-item');
    const lastIndex = STEPS.length - 1;

    expect(stepItems[lastIndex]).toHaveClass('active');
    expect(stepItems[lastIndex]).not.toHaveClass('completed');
    // All previous steps should be completed
    for (let i = 0; i < lastIndex; i++) {
      expect(stepItems[i]).toHaveClass('completed');
    }
  });
});
