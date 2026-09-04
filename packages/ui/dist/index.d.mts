import * as class_variance_authority_types from 'class-variance-authority/types';
import * as React from 'react';
import React__default, { ReactNode } from 'react';
import { VariantProps } from 'class-variance-authority';
import * as SelectPrimitive from '@radix-ui/react-select';
import * as CheckboxPrimitive from '@radix-ui/react-checkbox';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import * as ProgressPrimitive from '@radix-ui/react-progress';
import * as LabelPrimitive from '@radix-ui/react-label';
import * as SeparatorPrimitive from '@radix-ui/react-separator';
import * as TabsPrimitive from '@radix-ui/react-tabs';
import { ClassValue } from 'clsx';

declare const buttonVariants: (props?: ({
    variant?: "default" | "destructive" | "outline" | "secondary" | "ghost" | "link" | null | undefined;
    size?: "default" | "sm" | "lg" | "icon" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
type ButtonVariant = VariantProps<typeof buttonVariants>['variant'];
type ButtonSize = VariantProps<typeof buttonVariants>['size'];
interface ButtonProps extends React__default.ButtonHTMLAttributes<HTMLButtonElement>, Omit<VariantProps<typeof buttonVariants>, 'variant' | 'size'> {
    variant?: ButtonVariant | 'primary' | 'danger' | 'generate';
    size?: ButtonSize | 'md';
    asChild?: boolean;
    loading?: boolean;
}
declare const Button: React__default.ForwardRefExoticComponent<ButtonProps & React__default.RefAttributes<HTMLButtonElement>>;

interface InputProps extends React__default.InputHTMLAttributes<HTMLInputElement> {
    label?: string;
    helpText?: string;
    error?: string;
    onGenerate?: () => void;
}
declare const Input: React__default.ForwardRefExoticComponent<InputProps & React__default.RefAttributes<HTMLInputElement>>;

declare const SelectTrigger: React__default.ForwardRefExoticComponent<Omit<SelectPrimitive.SelectTriggerProps & React__default.RefAttributes<HTMLButtonElement>, "ref"> & React__default.RefAttributes<HTMLButtonElement>>;
declare const SelectContent: React__default.ForwardRefExoticComponent<Omit<SelectPrimitive.SelectContentProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & React__default.RefAttributes<HTMLDivElement>>;
declare const SelectLabel: React__default.ForwardRefExoticComponent<Omit<SelectPrimitive.SelectLabelProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & React__default.RefAttributes<HTMLDivElement>>;
declare const SelectItem: React__default.ForwardRefExoticComponent<Omit<SelectPrimitive.SelectItemProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & React__default.RefAttributes<HTMLDivElement>>;
declare const SelectSeparator: React__default.ForwardRefExoticComponent<Omit<SelectPrimitive.SelectSeparatorProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & React__default.RefAttributes<HTMLDivElement>>;
declare const SelectValue: React__default.ForwardRefExoticComponent<SelectPrimitive.SelectValueProps & React__default.RefAttributes<HTMLSpanElement>>;
interface SelectOption {
    value: string;
    label: string;
    disabled?: boolean;
}
interface SelectProps extends Omit<React__default.SelectHTMLAttributes<HTMLSelectElement>, 'onChange' | 'size' | 'value' | 'defaultValue'> {
    label?: string;
    options: SelectOption[];
    helpText?: string;
    error?: string;
    value?: string;
    defaultValue?: string;
    onChange?: (event: React__default.ChangeEvent<HTMLSelectElement>) => void;
    onValueChange?: (value: string) => void;
    placeholder?: string;
}
declare const Select: React__default.ForwardRefExoticComponent<SelectProps & React__default.RefAttributes<HTMLButtonElement>>;

interface CheckboxProps extends Omit<React__default.ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>, 'checked' | 'defaultChecked' | 'onCheckedChange' | 'onChange'> {
    label?: string;
    helpText?: string;
    checked?: boolean;
    defaultChecked?: boolean;
    onChange?: (event: React__default.ChangeEvent<HTMLInputElement>) => void;
    onCheckedChange?: (checked: boolean) => void;
}
declare const Checkbox: React__default.ForwardRefExoticComponent<CheckboxProps & React__default.RefAttributes<HTMLButtonElement>>;

interface ModalProps extends Omit<React__default.HTMLAttributes<HTMLDivElement>, 'title'> {
    isOpen: boolean;
    onClose?: () => void;
    title: string;
    size?: 'sm' | 'md' | 'lg' | 'xl' | 'full';
}
declare const Modal: React__default.ForwardRefExoticComponent<ModalProps & React__default.RefAttributes<HTMLDivElement>>;

interface InfoBoxProps extends React__default.HTMLAttributes<HTMLDivElement> {
    variant?: 'default' | 'info' | 'warning' | 'success' | 'error' | 'destructive';
}
declare const InfoBox: React__default.ForwardRefExoticComponent<InfoBoxProps & React__default.RefAttributes<HTMLDivElement>>;

interface ProgressBarProps extends React__default.HTMLAttributes<HTMLDivElement> {
    value: number;
    max?: number;
    label?: string;
    showPercentage?: boolean;
}
declare const ProgressBar: React__default.ForwardRefExoticComponent<ProgressBarProps & React__default.RefAttributes<HTMLDivElement>>;

declare const wizardVariants: (props?: ({
    size?: "sm" | "lg" | "md" | "xl" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface WizardStep {
    id: string;
    title: string;
    description?: string;
    icon?: React__default.ReactNode;
}
interface WizardProps extends Omit<React__default.HTMLAttributes<HTMLDivElement>, 'title'>, VariantProps<typeof wizardVariants> {
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
declare const Wizard: React__default.ForwardRefExoticComponent<WizardProps & React__default.RefAttributes<HTMLDivElement>>;

declare const alertVariants: (props?: ({
    variant?: "default" | "destructive" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
declare const Alert: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLDivElement> & VariantProps<(props?: ({
    variant?: "default" | "destructive" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string> & React__default.RefAttributes<HTMLDivElement>>;
declare const AlertTitle: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLHeadingElement> & React__default.RefAttributes<HTMLHeadingElement>>;
declare const AlertDescription: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLParagraphElement> & React__default.RefAttributes<HTMLParagraphElement>>;

declare const Dialog: React__default.FC<DialogPrimitive.DialogProps>;
declare const DialogTrigger: React__default.ForwardRefExoticComponent<DialogPrimitive.DialogTriggerProps & React__default.RefAttributes<HTMLButtonElement>>;
declare const DialogPortal: React__default.FC<DialogPrimitive.DialogPortalProps>;
declare const DialogClose: React__default.ForwardRefExoticComponent<DialogPrimitive.DialogCloseProps & React__default.RefAttributes<HTMLButtonElement>>;
declare const DialogOverlay: React__default.ForwardRefExoticComponent<Omit<DialogPrimitive.DialogOverlayProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & React__default.RefAttributes<HTMLDivElement>>;
declare const DialogContent: React__default.ForwardRefExoticComponent<Omit<DialogPrimitive.DialogContentProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & React__default.RefAttributes<HTMLDivElement>>;
declare const DialogHeader: ({ className, ...props }: React__default.HTMLAttributes<HTMLDivElement>) => React__default.JSX.Element;
declare const DialogFooter: ({ className, ...props }: React__default.HTMLAttributes<HTMLDivElement>) => React__default.JSX.Element;
declare const DialogTitle: React__default.ForwardRefExoticComponent<Omit<DialogPrimitive.DialogTitleProps & React__default.RefAttributes<HTMLHeadingElement>, "ref"> & React__default.RefAttributes<HTMLHeadingElement>>;
declare const DialogDescription: React__default.ForwardRefExoticComponent<Omit<DialogPrimitive.DialogDescriptionProps & React__default.RefAttributes<HTMLParagraphElement>, "ref"> & React__default.RefAttributes<HTMLParagraphElement>>;

declare const Progress: React__default.ForwardRefExoticComponent<Omit<ProgressPrimitive.ProgressProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & {
    value?: number;
} & React__default.RefAttributes<HTMLDivElement>>;

declare const Card: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLDivElement> & React__default.RefAttributes<HTMLDivElement>>;
declare const CardHeader: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLDivElement> & React__default.RefAttributes<HTMLDivElement>>;
declare const CardTitle: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLHeadingElement> & React__default.RefAttributes<HTMLHeadingElement>>;
declare const CardDescription: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLParagraphElement> & React__default.RefAttributes<HTMLParagraphElement>>;
declare const CardContent: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLDivElement> & React__default.RefAttributes<HTMLDivElement>>;
declare const CardFooter: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLDivElement> & React__default.RefAttributes<HTMLDivElement>>;

declare const Label: React__default.ForwardRefExoticComponent<Omit<LabelPrimitive.LabelProps & React__default.RefAttributes<HTMLLabelElement>, "ref"> & React__default.RefAttributes<HTMLLabelElement>>;

declare const Separator: React__default.ForwardRefExoticComponent<Omit<SeparatorPrimitive.SeparatorProps & React__default.RefAttributes<HTMLDivElement>, "ref"> & React__default.RefAttributes<HTMLDivElement>>;

interface SectionHeaderProps extends React__default.HTMLAttributes<HTMLDivElement> {
    title: string;
    size?: 'sm' | 'md';
}
declare const SectionHeader: React__default.ForwardRefExoticComponent<SectionHeaderProps & React__default.RefAttributes<HTMLDivElement>>;

declare const Table: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLTableElement> & React__default.RefAttributes<HTMLTableElement>>;
declare const TableHeader: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLTableSectionElement> & React__default.RefAttributes<HTMLTableSectionElement>>;
declare const TableBody: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLTableSectionElement> & React__default.RefAttributes<HTMLTableSectionElement>>;
declare const TableFooter: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLTableSectionElement> & React__default.RefAttributes<HTMLTableSectionElement>>;
declare const TableRow: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLTableRowElement> & React__default.RefAttributes<HTMLTableRowElement>>;
declare const TableHead: React__default.ForwardRefExoticComponent<React__default.ThHTMLAttributes<HTMLTableCellElement> & React__default.RefAttributes<HTMLTableCellElement>>;
declare const TableCell: React__default.ForwardRefExoticComponent<React__default.TdHTMLAttributes<HTMLTableCellElement> & React__default.RefAttributes<HTMLTableCellElement>>;
declare const TableCaption: React__default.ForwardRefExoticComponent<React__default.HTMLAttributes<HTMLTableCaptionElement> & React__default.RefAttributes<HTMLTableCaptionElement>>;

declare const badgeVariants: (props?: ({
    variant?: "default" | "destructive" | "outline" | "secondary" | "info" | "warning" | "success" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface BadgeProps extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof badgeVariants> {
}
declare function Badge({ className, variant, ...props }: BadgeProps): React.JSX.Element;

declare const Tabs: React.ForwardRefExoticComponent<TabsPrimitive.TabsProps & React.RefAttributes<HTMLDivElement>>;
declare const TabsList: React.ForwardRefExoticComponent<Omit<TabsPrimitive.TabsListProps & React.RefAttributes<HTMLDivElement>, "ref"> & React.RefAttributes<HTMLDivElement>>;
declare const TabsTrigger: React.ForwardRefExoticComponent<Omit<TabsPrimitive.TabsTriggerProps & React.RefAttributes<HTMLButtonElement>, "ref"> & React.RefAttributes<HTMLButtonElement>>;
declare const TabsContent: React.ForwardRefExoticComponent<Omit<TabsPrimitive.TabsContentProps & React.RefAttributes<HTMLDivElement>, "ref"> & React.RefAttributes<HTMLDivElement>>;

interface EmptyStateProps {
    /** Icon to display (SVG element or component) */
    icon?: ReactNode;
    /** Main heading text */
    title: string;
    /** Description text below the heading */
    description?: string;
    /** Call-to-action button or element */
    action?: ReactNode;
    /** Additional CSS classes */
    className?: string;
}
/**
 * EmptyState component for displaying placeholder content when a list or section is empty.
 * Provides a consistent layout with icon, title, description, and optional action button.
 */
declare function EmptyState({ icon, title, description, action, className }: EmptyStateProps): React.JSX.Element;

declare function cn(...inputs: ClassValue[]): string;

export { Alert, AlertDescription, AlertTitle, Badge, type BadgeProps, Button, type ButtonProps, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle, Checkbox, type CheckboxProps, Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogOverlay, DialogPortal, DialogTitle, DialogTrigger, EmptyState, type EmptyStateProps, InfoBox, type InfoBoxProps, Input, type InputProps, Label, Modal, type ModalProps, Progress, ProgressBar, type ProgressBarProps, SectionHeader, type SectionHeaderProps, Select, SelectContent, SelectItem, SelectLabel, type SelectOption, type SelectProps, SelectSeparator, SelectTrigger, SelectValue, Separator, Table, TableBody, TableCaption, TableCell, TableFooter, TableHead, TableHeader, TableRow, Tabs, TabsContent, TabsList, TabsTrigger, Wizard, type WizardProps, type WizardStep, alertVariants, badgeVariants, buttonVariants, cn, wizardVariants };
