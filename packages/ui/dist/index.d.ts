import React from 'react';

type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'ghost' | 'generate';
type ButtonSize = 'sm' | 'md' | 'lg';
interface ButtonProps extends React.ComponentPropsWithoutRef<'button'> {
    variant?: ButtonVariant;
    size?: ButtonSize;
    loading?: boolean;
}
declare const Button: React.ForwardRefExoticComponent<ButtonProps & React.RefAttributes<HTMLButtonElement>>;

interface InputProps extends React.ComponentPropsWithoutRef<'input'> {
    label: string;
    helpText?: string;
    error?: string;
    onGenerate?: () => void;
}
declare const Input: React.ForwardRefExoticComponent<InputProps & React.RefAttributes<HTMLInputElement>>;

interface SelectOption {
    value: string;
    label: string;
}
interface SelectProps extends React.ComponentPropsWithoutRef<'select'> {
    label: string;
    options: SelectOption[];
    helpText?: string;
    error?: string;
}
declare const Select: React.ForwardRefExoticComponent<SelectProps & React.RefAttributes<HTMLSelectElement>>;

interface CheckboxProps extends Omit<React.ComponentPropsWithoutRef<'input'>, 'type'> {
    label: string;
    helpText?: string;
}
declare const Checkbox: React.ForwardRefExoticComponent<CheckboxProps & React.RefAttributes<HTMLInputElement>>;

interface ModalProps extends Omit<React.ComponentPropsWithoutRef<'div'>, 'title'> {
    isOpen: boolean;
    onClose?: () => void;
    title: string;
}
declare const Modal: React.ForwardRefExoticComponent<ModalProps & React.RefAttributes<HTMLDivElement>>;

type InfoBoxVariant = 'info' | 'warning' | 'success' | 'error';
interface InfoBoxProps extends React.ComponentPropsWithoutRef<'div'> {
    variant?: InfoBoxVariant;
}
declare const InfoBox: React.ForwardRefExoticComponent<InfoBoxProps & React.RefAttributes<HTMLDivElement>>;

interface ProgressBarProps extends Omit<React.ComponentPropsWithoutRef<'div'>, 'children'> {
    value: number;
    max?: number;
    label?: string;
    showPercentage?: boolean;
    thin?: boolean;
}
declare const ProgressBar: React.ForwardRefExoticComponent<ProgressBarProps & React.RefAttributes<HTMLDivElement>>;

export { Button, type ButtonProps, type ButtonSize, type ButtonVariant, Checkbox, type CheckboxProps, InfoBox, type InfoBoxProps, type InfoBoxVariant, Input, type InputProps, Modal, type ModalProps, ProgressBar, type ProgressBarProps, Select, type SelectOption, type SelectProps };
