import * as class_variance_authority_types from 'class-variance-authority/types';
import React from 'react';
import { VariantProps } from 'class-variance-authority';
import { ClassValue } from 'clsx';

declare const buttonVariants: (props?: ({
    variant?: "primary" | "secondary" | "danger" | "ghost" | "generate" | null | undefined;
    size?: "sm" | "md" | "lg" | null | undefined;
    tone?: "default" | "success" | "warning" | "info" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
    asChild?: boolean;
    loading?: boolean;
}
declare const Button: React.ForwardRefExoticComponent<ButtonProps & React.RefAttributes<HTMLButtonElement>>;

declare const inputVariants: (props?: ({
    variant?: "default" | "error" | null | undefined;
    size?: "sm" | "md" | "lg" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface InputProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'size'>, VariantProps<typeof inputVariants> {
    label: string;
    helpText?: string;
    error?: string;
    onGenerate?: () => void;
}
declare const Input: React.ForwardRefExoticComponent<InputProps & React.RefAttributes<HTMLInputElement>>;

declare const selectVariants: (props?: ({
    variant?: "default" | "error" | null | undefined;
    size?: "sm" | "md" | "lg" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface SelectOption {
    value: string;
    label: string;
}
interface SelectProps extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'size'>, VariantProps<typeof selectVariants> {
    label: string;
    options: SelectOption[];
    helpText?: string;
    error?: string;
}
declare const Select: React.ForwardRefExoticComponent<SelectProps & React.RefAttributes<HTMLSelectElement>>;

declare const checkboxVariants: (props?: ({
    size?: "sm" | "md" | "lg" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface CheckboxProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'>, VariantProps<typeof checkboxVariants> {
    label: string;
    helpText?: string;
}
declare const Checkbox: React.ForwardRefExoticComponent<CheckboxProps & React.RefAttributes<HTMLInputElement>>;

declare const modalVariants: (props?: ({
    size?: "sm" | "md" | "lg" | "xl" | "full" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface ModalProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'>, VariantProps<typeof modalVariants> {
    isOpen: boolean;
    onClose?: () => void;
    title: string;
}
declare const Modal: React.ForwardRefExoticComponent<ModalProps & React.RefAttributes<HTMLDivElement>>;

declare const infoBoxVariants: (props?: ({
    variant?: "success" | "warning" | "info" | "error" | null | undefined;
    size?: "sm" | "md" | "lg" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface InfoBoxProps extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof infoBoxVariants> {
}
declare const InfoBox: React.ForwardRefExoticComponent<InfoBoxProps & React.RefAttributes<HTMLDivElement>>;

declare const progressBarVariants: (props?: ({
    size?: "sm" | "md" | "lg" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
declare const trackVariants: (props?: ({
    size?: "sm" | "md" | "lg" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
declare const fillVariants: (props?: ({
    variant?: "default" | "success" | "warning" | "error" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface ProgressBarProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'children'>, VariantProps<typeof progressBarVariants>, VariantProps<typeof fillVariants> {
    value: number;
    max?: number;
    label?: string;
    showPercentage?: boolean;
}
declare const ProgressBar: React.ForwardRefExoticComponent<ProgressBarProps & React.RefAttributes<HTMLDivElement>>;

declare const wizardVariants: (props?: ({
    size?: "sm" | "md" | "lg" | "xl" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface WizardStep {
    id: string;
    title: string;
    description?: string;
    icon?: React.ReactNode;
}
interface WizardProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'>, VariantProps<typeof wizardVariants> {
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
declare const Wizard: React.ForwardRefExoticComponent<WizardProps & React.RefAttributes<HTMLDivElement>>;

declare function cn(...inputs: ClassValue[]): string;

export { Button, type ButtonProps, Checkbox, type CheckboxProps, InfoBox, type InfoBoxProps, Input, type InputProps, Modal, type ModalProps, ProgressBar, type ProgressBarProps, Select, type SelectOption, type SelectProps, Wizard, type WizardProps, type WizardStep, buttonVariants, checkboxVariants, cn, fillVariants, infoBoxVariants, inputVariants, modalVariants, progressBarVariants, selectVariants, trackVariants, wizardVariants };
