import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { Input } from './Input';

describe('Input', () => {
  it('renders with label', () => {
    render(<Input label="Test Label" />);
    expect(screen.getByLabelText('Test Label')).toBeInTheDocument();
  });

  it('displays value', () => {
    render(<Input label="Test" value="test value" onChange={() => {}} />);
    expect(screen.getByDisplayValue('test value')).toBeInTheDocument();
  });

  it('calls onChange when value changes', () => {
    const onChange = jest.fn();
    render(<Input label="Test" onChange={onChange} />);

    fireEvent.change(screen.getByLabelText('Test'), { target: { value: 'new' } });
    expect(onChange).toHaveBeenCalled();
  });

  it('shows help text', () => {
    render(<Input label="Test" helpText="Help message" />);
    expect(screen.getByText('Help message')).toBeInTheDocument();
  });

  it('shows error instead of help text when error exists', () => {
    render(<Input label="Test" helpText="Help message" error="Error message" />);
    expect(screen.getByText('Error message')).toBeInTheDocument();
    expect(screen.queryByText('Help message')).not.toBeInTheDocument();
  });

  it('applies error class when error exists', () => {
    render(<Input label="Test" error="Error" />);
    const input = screen.getByLabelText('Test');
    expect(input).toHaveClass('input-error');
  });

  it('renders generate button when onGenerate provided', () => {
    const onGenerate = jest.fn();
    render(<Input label="Test" onGenerate={onGenerate} />);
    expect(screen.getByText('Generate')).toBeInTheDocument();
  });

  it('calls onGenerate when generate button clicked', () => {
    const onGenerate = jest.fn();
    render(<Input label="Test" onGenerate={onGenerate} />);

    fireEvent.click(screen.getByText('Generate'));
    expect(onGenerate).toHaveBeenCalled();
  });

  it('generates id from label', () => {
    render(<Input label="My Test Label" />);
    const input = screen.getByLabelText('My Test Label');
    expect(input).toHaveAttribute('id', 'my-test-label');
  });

  it('uses provided id over generated one', () => {
    render(<Input label="Test" id="custom-id" />);
    const input = screen.getByLabelText('Test');
    expect(input).toHaveAttribute('id', 'custom-id');
  });

  it('passes through additional props', () => {
    render(<Input label="Test" type="password" placeholder="Enter password" />);
    const input = screen.getByLabelText('Test');
    expect(input).toHaveAttribute('type', 'password');
    expect(input).toHaveAttribute('placeholder', 'Enter password');
  });
});
