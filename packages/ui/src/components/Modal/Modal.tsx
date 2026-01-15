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
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-lg',
  xl: 'max-w-xl',
  full: 'max-w-[90vw]',
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
