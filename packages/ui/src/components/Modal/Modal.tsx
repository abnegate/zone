import React, { forwardRef, useEffect } from 'react';

export interface ModalProps extends Omit<React.ComponentPropsWithoutRef<'div'>, 'title'> {
  isOpen: boolean;
  onClose?: () => void;
  title: string;
}

export const Modal = forwardRef<HTMLDivElement, ModalProps>(
  ({ isOpen, onClose, title, children, className = '', ...props }, ref) => {
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
      <div className="ui-modal-overlay" onClick={onClose}>
        <div
          ref={ref}
          className={`ui-modal ${className}`.trim()}
          onClick={e => e.stopPropagation()}
          {...props}
        >
          <h3 className="ui-modal__title">{title}</h3>
          {children}
        </div>
      </div>
    );
  }
);

Modal.displayName = 'Modal';
