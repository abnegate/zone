import React, { forwardRef, useCallback, useId, useRef } from 'react';
import * as SelectPrimitive from '@radix-ui/react-select';
import { cn } from '../../lib/utils';
import { Label } from '../Label';

const SelectTrigger = forwardRef<
  React.ElementRef<typeof SelectPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Trigger>
>(({ className, children, ...props }, ref) => (
  <SelectPrimitive.Trigger
    ref={ref}
    className={cn('ui-select-trigger', className)}
    {...props}
  >
    {children}
    <SelectPrimitive.Icon asChild>
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        className="ui-select-icon"
      >
        <polyline points="6 9 12 15 18 9" />
      </svg>
    </SelectPrimitive.Icon>
  </SelectPrimitive.Trigger>
));
SelectTrigger.displayName = SelectPrimitive.Trigger.displayName;

const SelectContent = forwardRef<
  React.ElementRef<typeof SelectPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Content>
>(({ className, children, position = 'popper', ...props }, ref) => (
  <SelectPrimitive.Portal>
    <SelectPrimitive.Content
      ref={ref}
      className={cn('ui-select-content', className)}
      position={position}
      {...props}
    >
      <SelectPrimitive.ScrollUpButton className="ui-select-scroll-button">
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className="ui-select-scroll-icon"
        >
          <polyline points="18 15 12 9 6 15" />
        </svg>
      </SelectPrimitive.ScrollUpButton>
      <SelectPrimitive.Viewport
        className={cn('ui-select-viewport', position === 'popper' && 'ui-select-viewport-popper')}
      >
        {children}
      </SelectPrimitive.Viewport>
      <SelectPrimitive.ScrollDownButton className="ui-select-scroll-button">
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className="ui-select-scroll-icon"
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </SelectPrimitive.ScrollDownButton>
    </SelectPrimitive.Content>
  </SelectPrimitive.Portal>
));
SelectContent.displayName = SelectPrimitive.Content.displayName;

const SelectLabel = forwardRef<
  React.ElementRef<typeof SelectPrimitive.Label>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Label>
>(({ className, ...props }, ref) => (
  <SelectPrimitive.Label
    ref={ref}
    className={cn('ui-select-label', className)}
    {...props}
  />
));
SelectLabel.displayName = SelectPrimitive.Label.displayName;

interface SelectItemProps extends React.ComponentPropsWithoutRef<typeof SelectPrimitive.Item> {
  description?: React.ReactNode;
}

const SelectItem = forwardRef<React.ElementRef<typeof SelectPrimitive.Item>, SelectItemProps>(
  ({ className, children, description, ...props }, ref) => {
    const descriptionId = useId();
    const described = description !== undefined && description !== null && description !== '';
    return (
      <SelectPrimitive.Item
        ref={ref}
        className={cn('ui-select-item', described && 'ui-select-item-described', className)}
        aria-describedby={described ? descriptionId : undefined}
        {...props}
      >
        <span className="ui-select-item-indicator">
          <SelectPrimitive.ItemIndicator>
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="3"
              strokeLinecap="round"
              strokeLinejoin="round"
              className="ui-select-item-icon"
            >
              <polyline points="20 6 9 17 4 12" />
            </svg>
          </SelectPrimitive.ItemIndicator>
        </span>
        <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
        {described && (
          <span id={descriptionId} className="ui-select-item-description">
            {description}
          </span>
        )}
      </SelectPrimitive.Item>
    );
  }
);
SelectItem.displayName = SelectPrimitive.Item.displayName;

const SelectSeparator = forwardRef<
  React.ElementRef<typeof SelectPrimitive.Separator>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Separator>
>(({ className, ...props }, ref) => (
  <SelectPrimitive.Separator
    ref={ref}
    className={cn('ui-select-separator', className)}
    {...props}
  />
));
SelectSeparator.displayName = SelectPrimitive.Separator.displayName;

const SelectValue = SelectPrimitive.Value;
const SelectRoot = SelectPrimitive.Root;

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  description?: string;
}

const SELECT_EMPTY_VALUE = '__empty__';

function unavailableText(options: readonly SelectOption[]): string {
  return options
    .filter((option) => option.disabled && option.description)
    .map((option) => `${option.label} is unavailable. ${option.description}`)
    .join(' ');
}

interface SelectUnavailableProps {
  id: string;
  options: readonly SelectOption[];
}

function SelectUnavailable({ id, options }: SelectUnavailableProps) {
  const text = unavailableText(options);
  return text === '' ? null : (
    <span id={id} className="sr-only">
      {text}
    </span>
  );
}

function unavailableId(options: readonly SelectOption[], id: string): string | undefined {
  return unavailableText(options) === '' ? undefined : id;
}

function encodeOptionValue(value: string): string {
  return value === '' ? SELECT_EMPTY_VALUE : value;
}

function decodeSelectValue(value: string): string {
  return value === SELECT_EMPTY_VALUE ? '' : value;
}

function encodeRootValue(value: string | undefined, hasEmptyOption: boolean): string | undefined {
  if (value === undefined) return undefined;
  if (value === '') return hasEmptyOption ? SELECT_EMPTY_VALUE : '';
  return value;
}

export interface SelectProps
  extends Omit<
    React.SelectHTMLAttributes<HTMLSelectElement>,
    'onChange' | 'size' | 'value' | 'defaultValue'
  > {
  label?: string;
  options: SelectOption[];
  helpText?: string;
  error?: string;
  value?: string;
  defaultValue?: string;
  onChange?: (event: React.ChangeEvent<HTMLSelectElement>) => void;
  onValueChange?: (value: string) => void;
  placeholder?: string;
  compact?: boolean;
  wrapperClassName?: string;
}

const Select = forwardRef<HTMLButtonElement, SelectProps>(
  (
    {
      label,
      options,
      helpText,
      error,
      id,
      className,
      value,
      defaultValue,
      onChange,
      onValueChange,
      name,
      disabled,
      required,
      placeholder,
      compact = false,
      wrapperClassName,
      title,
      'aria-label': ariaLabel,
    },
    ref
  ) => {
    const selectId =
      id || (label ? label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') : undefined);
    const placeholderText = placeholder ?? 'Select an option';
    const hasEmptyOption = options.some((option) => option.value === '');
    const valueRef = useRef(value);
    valueRef.current = value;
    const reasonsId = useId();

    const handleValueChange = useCallback(
      (nextValue: string) => {
        // Radix remounts its hidden form-bubble select whenever the option set
        // changes and echoes the current value back through onValueChange. No
        // item can hold an empty value, so an empty change is only ever that
        // echo, and forwarding it would clobber a value set while options load.
        if (nextValue === '') return;
        const decoded = decodeSelectValue(nextValue);
        if (valueRef.current !== undefined && decoded === valueRef.current) return;
        onValueChange?.(decoded);
        if (onChange) {
          const syntheticEvent = {
            target: { value: decoded, name },
            currentTarget: { value: decoded, name },
          } as React.ChangeEvent<HTMLSelectElement>;
          onChange(syntheticEvent);
        }
      },
      [name, onChange, onValueChange]
    );

    const suppressNativeChange = useCallback((event: React.ChangeEvent<HTMLDivElement>) => {
      // Hidden form-bubble <select> fires change when the controlled value is
      // set; that is not a user edit of a parent <form onChange>.
      event.stopPropagation();
    }, []);

    const control = (
      <div className="ui-select-control" onChange={suppressNativeChange}>
        <SelectPrimitive.Root
          value={encodeRootValue(value, hasEmptyOption)}
          defaultValue={encodeRootValue(defaultValue, hasEmptyOption)}
          onValueChange={handleValueChange}
          name={name}
          disabled={disabled}
          required={required}
        >
          <SelectTrigger
            id={selectId}
            ref={ref}
            className={cn(error && 'ui-select-trigger-error', className)}
            aria-label={ariaLabel}
            aria-describedby={unavailableId(options, reasonsId)}
            title={title}
          >
            <SelectValue placeholder={placeholderText} />
          </SelectTrigger>
          <SelectUnavailable id={reasonsId} options={options} />
          <SelectContent>
            {options.map((option) => (
              <SelectItem
                key={option.value === '' ? SELECT_EMPTY_VALUE : option.value}
                value={encodeOptionValue(option.value)}
                disabled={option.disabled}
                description={option.description}
                data-value={option.value}
              >
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </SelectPrimitive.Root>
      </div>
    );

    if (compact) {
      return control;
    }

    return (
      <div className={cn('ui-select-wrapper', wrapperClassName)}>
        {label && <Label htmlFor={selectId}>{label}</Label>}
        {control}
        {error && <p className="ui-select-error-text">{error}</p>}
        {helpText && !error && <p className="ui-select-help-text">{helpText}</p>}
      </div>
    );
  }
);

Select.displayName = 'Select';

export {
  Select,
  SelectContent,
  SelectItem,
  SelectLabel,
  SelectRoot,
  SelectSeparator,
  SelectTrigger,
  SelectUnavailable,
  SelectValue,
  unavailableId,
};
