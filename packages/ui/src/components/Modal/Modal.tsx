import React from 'react';
import { cn } from '../../lib/utils';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '../Dialog';

export interface ModalProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'> {
  isOpen: boolean;
  onClose?: () => void;
  title: string;
  size?: 'sm' | 'md' | 'lg' | 'xl' | 'full';
}

const SIZE_CLASS_MAP: Record<NonNullable<ModalProps['size']>, string> = {
  sm: 'ui-dialog--sm',
  md: 'ui-dialog--md',
  lg: 'ui-dialog--lg',
  xl: 'ui-dialog--xl',
  full: 'ui-dialog--full',
};

const Modal = React.forwardRef<HTMLDivElement, ModalProps>(
  ({ isOpen, onClose, title, size = 'md', children, className, ...props }, ref) => {
    return (
      <Dialog open={isOpen} onOpenChange={(open) => (!open ? onClose?.() : undefined)}>
        <DialogContent ref={ref} className={cn(SIZE_CLASS_MAP[size], className)} {...props}>
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
          </DialogHeader>
          {children}
        </DialogContent>
      </Dialog>
    );
  }
);

Modal.displayName = 'Modal';

export { Modal };
