import React, { forwardRef, useEffect } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const overlayVariants = cva([
  'fixed inset-0 z-50',
  'flex items-center justify-center',
  'bg-[var(--ui-overlay-medium)]',
  'backdrop-blur-sm',
  'animate-in fade-in duration-200',
]);

const modalVariants = cva(
  [
    'relative',
    'bg-[var(--ui-bg-elevated)]',
    'border border-[var(--ui-border)]',
    'rounded-[var(--ui-radius-xl)]',
    'shadow-[var(--ui-shadow-xl)]',
    'p-[var(--ui-space-6)]',
    'max-h-[85vh] overflow-auto',
    'animate-in zoom-in-95 fade-in duration-200',
  ],
  {
    variants: {
      size: {
        sm: 'w-full max-w-sm',
        md: 'w-full max-w-md',
        lg: 'w-full max-w-lg',
        xl: 'w-full max-w-xl',
        full: 'w-full max-w-[90vw]',
      },
    },
    defaultVariants: {
      size: 'md',
    },
  }
);

const titleVariants = cva([
  'text-[var(--ui-text-lg)] font-semibold',
  'text-[var(--ui-text-primary)]',
  'mb-[var(--ui-space-4)]',
]);

export interface ModalProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'>,
    VariantProps<typeof modalVariants> {
  isOpen: boolean;
  onClose?: () => void;
  title: string;
}

const Modal = forwardRef<HTMLDivElement, ModalProps>(
  ({ isOpen, onClose, title, children, className, size, ...props }, ref) => {
    useEffect(() => {
      const handleEscape = (e: KeyboardEvent) => {
        if (e.key === 'Escape' && onClose) {
          onClose();
        }
      };

      if (isOpen) {
        document.addEventListener('keydown', handleEscape);
        document.body.style.overflow = 'hidden';
      }

      return () => {
        document.removeEventListener('keydown', handleEscape);
        document.body.style.overflow = '';
      };
    }, [isOpen, onClose]);

    if (!isOpen) return null;

    return (
      <div
        className={cn(overlayVariants())}
        onClick={onClose}
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
      >
        <div
          ref={ref}
          className={cn(modalVariants({ size, className }))}
          onClick={e => e.stopPropagation()}
          {...props}
        >
          <h3 id="modal-title" className={cn(titleVariants())}>
            {title}
          </h3>
          {children}
        </div>
      </div>
    );
  }
);

Modal.displayName = 'Modal';

export { Modal, modalVariants };
