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
    expect(activeStep).toHaveAttribute('data-step', '3');
  });

  it('displays step numbers for non-completed steps', () => {
    render(<StepPills currentStep={4} onStepClick={onStepClick} />);

    expect(screen.getByText('4')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
  });

  it('displays checkmarks for completed steps', () => {
    render(<StepPills currentStep={4} onStepClick={onStepClick} />);

    const checkmarks = screen.getAllByLabelText('Step completed');
    expect(checkmarks.length).toBe(3);
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

  it('handles first step correctly', () => {
    render(<StepPills currentStep={1} onStepClick={onStepClick} />);

    const activeStep = screen.getByRole('button', { current: 'step' });
    expect(activeStep).toHaveAttribute('data-step', '1');
  });

  it('handles last step correctly', () => {
    render(<StepPills currentStep={STEPS.length} onStepClick={onStepClick} />);

    const activeStep = screen.getByRole('button', { current: 'step' });
    expect(activeStep).toHaveAttribute('data-step', String(STEPS.length));

    const checkmarks = screen.getAllByLabelText('Step completed');
    expect(checkmarks.length).toBe(STEPS.length - 1);
  });
});
