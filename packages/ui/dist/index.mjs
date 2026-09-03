// src/components/Button/Button.tsx
import { forwardRef } from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva } from "class-variance-authority";

// src/lib/utils.ts
import { clsx } from "clsx";
import { twMerge } from "tailwind-merge";
function cn(...inputs) {
  return twMerge(clsx(inputs));
}

// src/components/Button/Button.tsx
import { jsx, jsxs } from "react/jsx-runtime";
var buttonVariants = cva(
  "ui-btn",
  {
    variants: {
      variant: {
        default: "ui-btn-primary",
        destructive: "ui-btn-destructive",
        outline: "ui-btn-outline",
        secondary: "ui-btn-secondary",
        ghost: "ui-btn-ghost",
        link: "ui-btn-link"
      },
      size: {
        default: "ui-btn-md",
        sm: "ui-btn-sm",
        lg: "ui-btn-lg",
        icon: "ui-btn-icon"
      }
    },
    defaultVariants: {
      variant: "default",
      size: "default"
    }
  }
);
var LEGACY_VARIANT_MAP = {
  primary: "default",
  danger: "destructive",
  generate: "secondary"
};
var LEGACY_SIZE_MAP = {
  md: "default"
};
var Button = forwardRef(
  ({
    className,
    variant,
    size,
    asChild = false,
    loading = false,
    disabled,
    children,
    type = "button",
    ...props
  }, ref) => {
    const Comp = asChild ? Slot : "button";
    const resolvedVariant = LEGACY_VARIANT_MAP[variant ?? ""] ?? variant;
    const resolvedSize = LEGACY_SIZE_MAP[size ?? ""] ?? size;
    return /* @__PURE__ */ jsxs(
      Comp,
      {
        ref,
        className: cn(buttonVariants({ variant: resolvedVariant, size: resolvedSize, className })),
        disabled: disabled || loading,
        type: asChild ? void 0 : type,
        ...props,
        children: [
          loading && /* @__PURE__ */ jsx("span", { className: "ui-btn-spinner", "aria-hidden": "true" }),
          children
        ]
      }
    );
  }
);
Button.displayName = "Button";

// src/components/Input/Input.tsx
import { forwardRef as forwardRef2 } from "react";

// src/components/Label/Label.tsx
import React2 from "react";
import * as LabelPrimitive from "@radix-ui/react-label";
import { jsx as jsx2 } from "react/jsx-runtime";
var Label = React2.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx2(LabelPrimitive.Root, { ref, className: cn("ui-label", className), ...props }));
Label.displayName = LabelPrimitive.Root.displayName;

// src/components/Input/Input.tsx
import { jsx as jsx3, jsxs as jsxs2 } from "react/jsx-runtime";
var Input = forwardRef2(
  ({ label, helpText, error, onGenerate, id, className, type = "text", ...props }, ref) => {
    const inputId = id || (label ? label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") : void 0);
    const describedBy = error ? `${inputId}-error` : helpText ? `${inputId}-help` : void 0;
    const inputElement = /* @__PURE__ */ jsx3(
      "input",
      {
        ref,
        id: inputId,
        className: cn("ui-input", error && "ui-input-error", className),
        type,
        "aria-invalid": !!error,
        "aria-describedby": describedBy,
        ...props
      }
    );
    return /* @__PURE__ */ jsxs2("div", { className: "ui-input-wrapper", children: [
      label && /* @__PURE__ */ jsx3(Label, { htmlFor: inputId, children: label }),
      onGenerate ? /* @__PURE__ */ jsxs2("div", { className: "ui-input-with-button", children: [
        inputElement,
        /* @__PURE__ */ jsx3(Button, { variant: "secondary", type: "button", onClick: onGenerate, children: "Generate" })
      ] }) : inputElement,
      error && /* @__PURE__ */ jsx3("p", { id: `${inputId}-error`, className: "ui-input-error-text", role: "alert", children: error }),
      helpText && !error && /* @__PURE__ */ jsx3("p", { id: `${inputId}-help`, className: "ui-input-help-text", children: helpText })
    ] });
  }
);
Input.displayName = "Input";

// src/components/Select/Select.tsx
import { forwardRef as forwardRef3, useCallback } from "react";
import * as SelectPrimitive from "@radix-ui/react-select";
import { jsx as jsx4, jsxs as jsxs3 } from "react/jsx-runtime";
var SelectTrigger = forwardRef3(({ className, children, ...props }, ref) => /* @__PURE__ */ jsxs3(
  SelectPrimitive.Trigger,
  {
    ref,
    className: cn("ui-select-trigger", className),
    ...props,
    children: [
      children,
      /* @__PURE__ */ jsx4(SelectPrimitive.Icon, { asChild: true, children: /* @__PURE__ */ jsx4(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "2",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-icon",
          children: /* @__PURE__ */ jsx4("polyline", { points: "6 9 12 15 18 9" })
        }
      ) })
    ]
  }
));
SelectTrigger.displayName = SelectPrimitive.Trigger.displayName;
var SelectContent = forwardRef3(({ className, children, position = "popper", ...props }, ref) => /* @__PURE__ */ jsx4(SelectPrimitive.Portal, { children: /* @__PURE__ */ jsxs3(
  SelectPrimitive.Content,
  {
    ref,
    className: cn("ui-select-content", className),
    position,
    ...props,
    children: [
      /* @__PURE__ */ jsx4(SelectPrimitive.ScrollUpButton, { className: "ui-select-scroll-button", children: /* @__PURE__ */ jsx4(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "2",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-scroll-icon",
          children: /* @__PURE__ */ jsx4("polyline", { points: "18 15 12 9 6 15" })
        }
      ) }),
      /* @__PURE__ */ jsx4(
        SelectPrimitive.Viewport,
        {
          className: cn("ui-select-viewport", position === "popper" && "ui-select-viewport-popper"),
          children
        }
      ),
      /* @__PURE__ */ jsx4(SelectPrimitive.ScrollDownButton, { className: "ui-select-scroll-button", children: /* @__PURE__ */ jsx4(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "2",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-scroll-icon",
          children: /* @__PURE__ */ jsx4("polyline", { points: "6 9 12 15 18 9" })
        }
      ) })
    ]
  }
) }));
SelectContent.displayName = SelectPrimitive.Content.displayName;
var SelectLabel = forwardRef3(({ className, ...props }, ref) => /* @__PURE__ */ jsx4(
  SelectPrimitive.Label,
  {
    ref,
    className: cn("ui-select-label", className),
    ...props
  }
));
SelectLabel.displayName = SelectPrimitive.Label.displayName;
var SelectItem = forwardRef3(({ className, children, ...props }, ref) => /* @__PURE__ */ jsxs3(
  SelectPrimitive.Item,
  {
    ref,
    className: cn("ui-select-item", className),
    ...props,
    children: [
      /* @__PURE__ */ jsx4("span", { className: "ui-select-item-indicator", children: /* @__PURE__ */ jsx4(SelectPrimitive.ItemIndicator, { children: /* @__PURE__ */ jsx4(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "3",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-item-icon",
          children: /* @__PURE__ */ jsx4("polyline", { points: "20 6 9 17 4 12" })
        }
      ) }) }),
      /* @__PURE__ */ jsx4(SelectPrimitive.ItemText, { children })
    ]
  }
));
SelectItem.displayName = SelectPrimitive.Item.displayName;
var SelectSeparator = forwardRef3(({ className, ...props }, ref) => /* @__PURE__ */ jsx4(
  SelectPrimitive.Separator,
  {
    ref,
    className: cn("ui-select-separator", className),
    ...props
  }
));
SelectSeparator.displayName = SelectPrimitive.Separator.displayName;
var SelectValue = SelectPrimitive.Value;
var Select = forwardRef3(
  ({
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
    placeholder
  }, ref) => {
    const selectId = id || (label ? label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") : void 0);
    const placeholderText = placeholder ?? "Select an option";
    const handleValueChange = useCallback(
      (nextValue) => {
        onValueChange?.(nextValue);
        if (onChange) {
          const syntheticEvent = {
            target: { value: nextValue, name },
            currentTarget: { value: nextValue, name }
          };
          onChange(syntheticEvent);
        }
      },
      [name, onChange, onValueChange]
    );
    return /* @__PURE__ */ jsxs3("div", { className: "ui-select-wrapper", children: [
      label && /* @__PURE__ */ jsx4(Label, { htmlFor: selectId, children: label }),
      /* @__PURE__ */ jsxs3(
        SelectPrimitive.Root,
        {
          value,
          defaultValue,
          onValueChange: handleValueChange,
          name,
          disabled,
          required,
          children: [
            /* @__PURE__ */ jsx4(
              SelectTrigger,
              {
                id: selectId,
                ref,
                className: cn(error && "ui-select-trigger-error", className),
                children: /* @__PURE__ */ jsx4(SelectValue, { placeholder: placeholderText })
              }
            ),
            /* @__PURE__ */ jsx4(SelectContent, { children: options.filter((option) => option.value !== "").map((option) => /* @__PURE__ */ jsx4(SelectItem, { value: option.value, disabled: option.disabled, children: option.label }, option.value)) })
          ]
        }
      ),
      error && /* @__PURE__ */ jsx4("p", { className: "ui-select-error-text", children: error }),
      helpText && !error && /* @__PURE__ */ jsx4("p", { className: "ui-select-help-text", children: helpText })
    ] });
  }
);
Select.displayName = "Select";

// src/components/Checkbox/Checkbox.tsx
import { forwardRef as forwardRef4, useCallback as useCallback2 } from "react";
import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { jsx as jsx5, jsxs as jsxs4 } from "react/jsx-runtime";
var Checkbox = forwardRef4(
  ({
    label,
    helpText,
    id,
    className,
    checked,
    defaultChecked,
    disabled,
    name,
    value,
    onChange,
    onCheckedChange,
    ...props
  }, ref) => {
    const checkboxId = id || (label ? label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") : void 0);
    const handleCheckedChange = useCallback2(
      (nextChecked) => {
        const resolvedChecked = nextChecked === true;
        onCheckedChange?.(resolvedChecked);
        if (onChange) {
          const syntheticEvent = {
            target: { checked: resolvedChecked, name, value },
            currentTarget: { checked: resolvedChecked, name, value }
          };
          onChange(syntheticEvent);
        }
      },
      [name, onChange, onCheckedChange, value]
    );
    return /* @__PURE__ */ jsxs4("div", { className: "ui-checkbox-wrapper", children: [
      /* @__PURE__ */ jsxs4("div", { className: "ui-checkbox-row", children: [
        /* @__PURE__ */ jsx5(
          CheckboxPrimitive.Root,
          {
            ref,
            id: checkboxId,
            className: cn("ui-checkbox", className),
            checked,
            defaultChecked,
            disabled,
            name,
            value,
            onCheckedChange: handleCheckedChange,
            ...props,
            children: /* @__PURE__ */ jsx5(CheckboxPrimitive.Indicator, { className: "ui-checkbox-indicator", children: /* @__PURE__ */ jsx5(
              "svg",
              {
                viewBox: "0 0 24 24",
                fill: "none",
                stroke: "currentColor",
                strokeWidth: "3",
                strokeLinecap: "round",
                strokeLinejoin: "round",
                className: "ui-checkbox-icon",
                children: /* @__PURE__ */ jsx5("polyline", { points: "20 6 9 17 4 12" })
              }
            ) })
          }
        ),
        label && /* @__PURE__ */ jsx5(Label, { htmlFor: checkboxId, children: label })
      ] }),
      helpText && /* @__PURE__ */ jsx5("p", { className: "ui-checkbox-help-text", children: helpText })
    ] });
  }
);
Checkbox.displayName = "Checkbox";

// src/components/Modal/Modal.tsx
import React7 from "react";

// src/components/Dialog/Dialog.tsx
import React6 from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { jsx as jsx6, jsxs as jsxs5 } from "react/jsx-runtime";
var Dialog = DialogPrimitive.Root;
var DialogTrigger = DialogPrimitive.Trigger;
var DialogPortal = DialogPrimitive.Portal;
var DialogClose = DialogPrimitive.Close;
var DialogOverlay = React6.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx6(
  DialogPrimitive.Overlay,
  {
    ref,
    className: cn("ui-dialog-overlay", className),
    ...props
  }
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;
var DialogContent = React6.forwardRef(({ className, children, ...props }, ref) => /* @__PURE__ */ jsxs5(DialogPortal, { children: [
  /* @__PURE__ */ jsx6(DialogOverlay, {}),
  /* @__PURE__ */ jsxs5(
    DialogPrimitive.Content,
    {
      ref,
      className: cn("ui-dialog-content", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsxs5(DialogPrimitive.Close, { className: "ui-dialog-close", children: [
          /* @__PURE__ */ jsx6(
            "svg",
            {
              viewBox: "0 0 24 24",
              fill: "none",
              stroke: "currentColor",
              strokeWidth: "2",
              strokeLinecap: "round",
              strokeLinejoin: "round",
              className: "ui-dialog-close-icon",
              children: /* @__PURE__ */ jsx6("path", { d: "M18 6L6 18M6 6l12 12" })
            }
          ),
          /* @__PURE__ */ jsx6("span", { className: "sr-only", children: "Close" })
        ] })
      ]
    }
  )
] }));
DialogContent.displayName = DialogPrimitive.Content.displayName;
var DialogHeader = ({ className, ...props }) => /* @__PURE__ */ jsx6("div", { className: cn("ui-dialog-header", className), ...props });
var DialogFooter = ({ className, ...props }) => /* @__PURE__ */ jsx6("div", { className: cn("ui-dialog-footer", className), ...props });
var DialogTitle = React6.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx6(
  DialogPrimitive.Title,
  {
    ref,
    className: cn("ui-dialog-title", className),
    ...props
  }
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;
var DialogDescription = React6.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx6(
  DialogPrimitive.Description,
  {
    ref,
    className: cn("ui-dialog-description", className),
    ...props
  }
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

// src/components/Modal/Modal.tsx
import { jsx as jsx7, jsxs as jsxs6 } from "react/jsx-runtime";
var SIZE_CLASS_MAP = {
  sm: "max-w-sm",
  md: "max-w-md",
  lg: "max-w-lg",
  xl: "max-w-xl",
  full: "max-w-[90vw]"
};
var Modal = React7.forwardRef(
  ({ isOpen, onClose, title, size = "md", children, className, ...props }, ref) => {
    return /* @__PURE__ */ jsx7(Dialog, { open: isOpen, onOpenChange: (open) => !open ? onClose?.() : void 0, children: /* @__PURE__ */ jsxs6(DialogContent, { ref, className: cn(SIZE_CLASS_MAP[size], className), ...props, children: [
      /* @__PURE__ */ jsx7(DialogHeader, { children: /* @__PURE__ */ jsx7(DialogTitle, { children: title }) }),
      children
    ] }) });
  }
);
Modal.displayName = "Modal";

// src/components/InfoBox/InfoBox.tsx
import React9 from "react";

// src/components/Alert/Alert.tsx
import React8 from "react";
import { cva as cva2 } from "class-variance-authority";
import { jsx as jsx8 } from "react/jsx-runtime";
var alertVariants = cva2(
  [
    "relative w-full rounded-lg border border-border bg-background p-4",
    "[&>svg+div]:translate-y-[-3px] [&>svg]:absolute [&>svg]:left-4 [&>svg]:top-4 [&>svg]:text-foreground",
    "[&>svg~*]:pl-7"
  ],
  {
    variants: {
      variant: {
        default: "text-foreground",
        destructive: "border-destructive/50 text-destructive dark:border-destructive [&>svg]:text-destructive"
      }
    },
    defaultVariants: {
      variant: "default"
    }
  }
);
var Alert = React8.forwardRef(({ className, variant, ...props }, ref) => /* @__PURE__ */ jsx8("div", { ref, role: "alert", className: cn(alertVariants({ variant }), className), ...props }));
Alert.displayName = "Alert";
var AlertTitle = React8.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx8("h5", { ref, className: cn("mb-1 font-medium leading-none tracking-tight", className), ...props })
);
AlertTitle.displayName = "AlertTitle";
var AlertDescription = React8.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx8("div", { ref, className: cn("text-sm [&_p]:leading-relaxed", className), ...props })
);
AlertDescription.displayName = "AlertDescription";

// src/components/InfoBox/InfoBox.tsx
import { jsx as jsx9 } from "react/jsx-runtime";
var INFOBOX_VARIANT_MAP = {
  default: "default",
  info: "default",
  success: "default",
  warning: "destructive",
  error: "destructive",
  destructive: "destructive"
};
var InfoBox = React9.forwardRef(
  ({ variant = "default", className, ...props }, ref) => {
    const mappedVariant = INFOBOX_VARIANT_MAP[variant] ?? "default";
    return /* @__PURE__ */ jsx9(Alert, { ref, variant: mappedVariant, className, ...props });
  }
);
InfoBox.displayName = "InfoBox";

// src/components/ProgressBar/ProgressBar.tsx
import React11 from "react";

// src/components/Progress/Progress.tsx
import React10 from "react";
import * as ProgressPrimitive from "@radix-ui/react-progress";
import { jsx as jsx10 } from "react/jsx-runtime";
var Progress = React10.forwardRef(({ className, value = 0, ...props }, ref) => /* @__PURE__ */ jsx10(
  ProgressPrimitive.Root,
  {
    ref,
    className: cn("relative h-2 w-full overflow-hidden rounded-full bg-secondary", className),
    ...props,
    children: /* @__PURE__ */ jsx10(
      ProgressPrimitive.Indicator,
      {
        className: "h-full w-full flex-1 bg-primary transition-all",
        style: { transform: `translateX(-${100 - Math.min(100, Math.max(0, value))}%)` }
      }
    )
  }
));
Progress.displayName = "Progress";

// src/components/ProgressBar/ProgressBar.tsx
import { jsx as jsx11, jsxs as jsxs7 } from "react/jsx-runtime";
var ProgressBar = React11.forwardRef(
  ({ value, max = 100, label, showPercentage = true, className, ...props }, ref) => {
    const percentage = Math.min(100, Math.max(0, Math.round(value / max * 100)));
    return /* @__PURE__ */ jsxs7("div", { ref, className: cn("grid gap-2", className), ...props, children: [
      (label || showPercentage) && /* @__PURE__ */ jsxs7("div", { className: "flex items-center justify-between text-sm text-muted-foreground", children: [
        /* @__PURE__ */ jsx11("span", { children: label }),
        showPercentage && /* @__PURE__ */ jsxs7("span", { children: [
          percentage,
          "%"
        ] })
      ] }),
      /* @__PURE__ */ jsx11(Progress, { value: percentage, "aria-label": label || "Progress" })
    ] });
  }
);
ProgressBar.displayName = "ProgressBar";

// src/components/Wizard/Wizard.tsx
import { forwardRef as forwardRef5, useEffect, useCallback as useCallback3, useState } from "react";
import { createPortal } from "react-dom";
import { cva as cva3 } from "class-variance-authority";
import { jsx as jsx12, jsxs as jsxs8 } from "react/jsx-runtime";
var overlayVariants = cva3([
  "fixed inset-0 z-50",
  "flex items-center justify-center",
  "bg-[var(--ui-overlay-medium)]",
  "backdrop-blur-sm"
]);
var wizardVariants = cva3(
  [
    "relative flex flex-col",
    "bg-[var(--ui-bg-elevated)]",
    "border border-[var(--ui-border)]",
    "rounded-[var(--ui-radius-xl)]",
    "shadow-[var(--ui-shadow-xl)]",
    "max-h-[90vh] overflow-hidden"
  ],
  {
    variants: {
      size: {
        sm: "w-full max-w-lg",
        md: "w-full max-w-2xl",
        lg: "w-full max-w-4xl",
        xl: "w-full max-w-6xl"
      }
    },
    defaultVariants: {
      size: "md"
    }
  }
);
var headerVariants = cva3([
  "flex items-start justify-between gap-[var(--ui-space-4)]",
  "p-[var(--ui-space-6)]",
  "border-b border-[var(--ui-border)]"
]);
var titleVariants = cva3([
  "text-[var(--ui-text-xl)] font-semibold",
  "text-[var(--ui-text-primary)]"
]);
var subtitleVariants = cva3([
  "mt-[var(--ui-space-1)]",
  "text-[var(--ui-text-sm)]",
  "text-[var(--ui-text-muted)]"
]);
var closeButtonVariants = cva3([
  "flex items-center justify-center",
  "w-8 h-8",
  "rounded-[var(--ui-radius-md)]",
  "text-[var(--ui-text-muted)]",
  "hover:bg-[var(--ui-bg-hover)] hover:text-[var(--ui-text-primary)]",
  "transition-colors duration-[var(--ui-duration-fast)]",
  "disabled:opacity-50 disabled:cursor-not-allowed"
]);
var stepsNavVariants = cva3([
  "px-[var(--ui-space-6)] py-[var(--ui-space-4)]",
  "border-b border-[var(--ui-border)]",
  "bg-[var(--ui-bg-surface)]"
]);
var progressTrackVariants = cva3([
  "h-1 w-full",
  "bg-[var(--ui-bg-muted)]",
  "rounded-full",
  "mb-[var(--ui-space-4)]",
  "overflow-hidden"
]);
var progressFillVariants = cva3([
  "h-full",
  "bg-gradient-to-r from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]",
  "rounded-full",
  "transition-all duration-[var(--ui-duration-normal)] ease-out"
]);
var stepListVariants = cva3([
  "flex items-center justify-between gap-[var(--ui-space-2)]",
  "list-none m-0 p-0"
]);
var stepItemVariants = cva3(
  ["flex-1"],
  {
    variants: {
      state: {
        completed: "",
        current: "",
        upcoming: ""
      },
      clickable: {
        true: "cursor-pointer",
        false: ""
      }
    },
    defaultVariants: {
      state: "upcoming",
      clickable: false
    }
  }
);
var stepButtonVariants = cva3(
  [
    "flex items-center gap-[var(--ui-space-3)] w-full",
    "p-[var(--ui-space-2)]",
    "rounded-[var(--ui-radius-md)]",
    "transition-colors duration-[var(--ui-duration-fast)]",
    "disabled:cursor-not-allowed"
  ],
  {
    variants: {
      state: {
        completed: "hover:bg-[var(--ui-bg-hover)]",
        current: "bg-[var(--ui-accent-muted)]",
        upcoming: "opacity-50"
      },
      clickable: {
        true: "hover:bg-[var(--ui-bg-hover)]",
        false: ""
      }
    },
    defaultVariants: {
      state: "upcoming",
      clickable: false
    }
  }
);
var stepIndicatorVariants = cva3(
  [
    "flex items-center justify-center shrink-0",
    "w-8 h-8",
    "rounded-full",
    "text-[var(--ui-text-sm)] font-medium",
    "transition-colors duration-[var(--ui-duration-fast)]"
  ],
  {
    variants: {
      state: {
        completed: "bg-[var(--ui-accent-500)] text-white",
        current: "bg-[var(--ui-accent-500)] text-white",
        upcoming: "bg-[var(--ui-bg-muted)] text-[var(--ui-text-muted)]"
      }
    },
    defaultVariants: {
      state: "upcoming"
    }
  }
);
var stepTitleVariants = cva3(
  ["text-[var(--ui-text-sm)] font-medium"],
  {
    variants: {
      state: {
        completed: "text-[var(--ui-text-primary)]",
        current: "text-[var(--ui-text-primary)]",
        upcoming: "text-[var(--ui-text-muted)]"
      }
    },
    defaultVariants: {
      state: "upcoming"
    }
  }
);
var stepDescriptionVariants = cva3([
  "text-[var(--ui-text-xs)]",
  "text-[var(--ui-text-muted)]"
]);
var contentVariants = cva3(
  [
    "flex-1 overflow-auto",
    "p-[var(--ui-space-6)]",
    "transition-all duration-150 ease-out"
  ],
  {
    variants: {
      animating: {
        next: "opacity-0 -translate-x-4",
        prev: "opacity-0 translate-x-4",
        none: "opacity-100 translate-x-0"
      }
    },
    defaultVariants: {
      animating: "none"
    }
  }
);
var footerVariants = cva3([
  "flex items-center justify-between",
  "p-[var(--ui-space-6)]",
  "border-t border-[var(--ui-border)]",
  "bg-[var(--ui-bg-surface)]"
]);
var Wizard = forwardRef5(
  ({
    isOpen,
    onClose,
    title,
    subtitle,
    steps,
    currentStep,
    onStepChange,
    onComplete,
    onCancel,
    completeLabel = "Complete",
    nextLabel = "Next",
    previousLabel = "Previous",
    cancelLabel = "Cancel",
    loading = false,
    canProceed = true,
    showStepNumbers = true,
    allowStepClick = false,
    size,
    children,
    className,
    ...props
  }, ref) => {
    const [animatingStep, setAnimatingStep] = useState(null);
    useEffect(() => {
      if (!isOpen) return void 0;
      const handleEscape = (e) => {
        if (e.key === "Escape" && onClose) {
          onClose();
        }
      };
      const scrollbarWidth = window.innerWidth - document.documentElement.clientWidth;
      const previousOverflow = document.body.style.overflow;
      const previousPaddingRight = document.body.style.paddingRight;
      document.addEventListener("keydown", handleEscape);
      document.body.style.overflow = "hidden";
      if (scrollbarWidth > 0) {
        document.body.style.paddingRight = `${scrollbarWidth}px`;
      }
      return () => {
        document.removeEventListener("keydown", handleEscape);
        document.body.style.overflow = previousOverflow;
        document.body.style.paddingRight = previousPaddingRight;
      };
    }, [isOpen, onClose]);
    const handleNext = useCallback3(() => {
      if (currentStep < steps.length - 1 && canProceed && !loading) {
        setAnimatingStep("next");
        setTimeout(() => {
          onStepChange?.(currentStep + 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, steps.length, canProceed, loading, onStepChange]);
    const handlePrevious = useCallback3(() => {
      if (currentStep > 0 && !loading) {
        setAnimatingStep("prev");
        setTimeout(() => {
          onStepChange?.(currentStep - 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, loading, onStepChange]);
    const handleStepClick = useCallback3(
      (stepIndex) => {
        if (!allowStepClick || loading) return;
        if (stepIndex < currentStep) {
          setAnimatingStep("prev");
          setTimeout(() => {
            onStepChange?.(stepIndex);
            setAnimatingStep(null);
          }, 150);
        } else if (stepIndex > currentStep && canProceed) {
          setAnimatingStep("next");
          setTimeout(() => {
            onStepChange?.(stepIndex);
            setAnimatingStep(null);
          }, 150);
        }
      },
      [allowStepClick, loading, currentStep, canProceed, onStepChange]
    );
    const handleComplete = useCallback3(() => {
      if (canProceed && !loading) {
        onComplete?.();
      }
    }, [canProceed, loading, onComplete]);
    const handleCancel = useCallback3(() => {
      if (!loading) {
        onCancel?.();
        onClose?.();
      }
    }, [loading, onCancel, onClose]);
    if (!isOpen) return null;
    const isLastStep = currentStep === steps.length - 1;
    const isFirstStep = currentStep === 0;
    const progressPercent = (currentStep + 1) / steps.length * 100;
    const getStepState = (index) => {
      if (index < currentStep) return "completed";
      if (index === currentStep) return "current";
      return "upcoming";
    };
    const dialog = /* @__PURE__ */ jsx12("div", { className: cn("ui-wizard-overlay", overlayVariants()), onClick: onClose, children: /* @__PURE__ */ jsxs8(
      "div",
      {
        ref,
        className: cn("ui-wizard", `ui-wizard--${size ?? "md"}`, wizardVariants({ size, className })),
        onClick: (e) => e.stopPropagation(),
        role: "dialog",
        "aria-modal": "true",
        "aria-labelledby": "wizard-title",
        ...props,
        children: [
          /* @__PURE__ */ jsxs8("header", { className: cn("ui-wizard-header", headerVariants()), children: [
            /* @__PURE__ */ jsxs8("div", { children: [
              /* @__PURE__ */ jsx12("h2", { id: "wizard-title", className: cn(titleVariants()), children: title }),
              subtitle && /* @__PURE__ */ jsx12("p", { className: cn("ui-wizard-subtitle", subtitleVariants()), children: subtitle })
            ] }),
            onClose && /* @__PURE__ */ jsx12(
              "button",
              {
                type: "button",
                className: cn("ui-wizard-close", closeButtonVariants()),
                onClick: onClose,
                "aria-label": "Close wizard",
                disabled: loading,
                children: /* @__PURE__ */ jsx12(
                  "svg",
                  {
                    className: "w-5 h-5",
                    viewBox: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    strokeWidth: "2",
                    strokeLinecap: "round",
                    strokeLinejoin: "round",
                    children: /* @__PURE__ */ jsx12("path", { d: "M18 6L6 18M6 6l12 12" })
                  }
                )
              }
            )
          ] }),
          /* @__PURE__ */ jsxs8("nav", { className: cn("ui-wizard-steps", stepsNavVariants()), "aria-label": "Wizard steps", children: [
            /* @__PURE__ */ jsx12("div", { className: cn("ui-wizard-progress", progressTrackVariants()), children: /* @__PURE__ */ jsx12(
              "div",
              {
                className: cn(progressFillVariants()),
                style: { width: `${progressPercent}%` }
              }
            ) }),
            /* @__PURE__ */ jsx12("ol", { className: cn(stepListVariants()), children: steps.map((step, index) => {
              const state = getStepState(index);
              const isClickable = allowStepClick && (state === "completed" || canProceed && index === currentStep + 1);
              return /* @__PURE__ */ jsx12(
                "li",
                {
                  className: cn(stepItemVariants({ state, clickable: isClickable })),
                  children: /* @__PURE__ */ jsxs8(
                    "button",
                    {
                      type: "button",
                      className: cn(stepButtonVariants({ state, clickable: isClickable })),
                      onClick: () => handleStepClick(index),
                      disabled: !isClickable || loading,
                      "aria-current": state === "current" ? "step" : void 0,
                      children: [
                        /* @__PURE__ */ jsx12("span", { className: cn(stepIndicatorVariants({ state })), children: state === "completed" ? /* @__PURE__ */ jsx12(
                          "svg",
                          {
                            className: "w-4 h-4",
                            viewBox: "0 0 24 24",
                            fill: "none",
                            stroke: "currentColor",
                            strokeWidth: "3",
                            strokeLinecap: "round",
                            strokeLinejoin: "round",
                            children: /* @__PURE__ */ jsx12("polyline", { points: "20 6 9 17 4 12" })
                          }
                        ) : step.icon ? step.icon : showStepNumbers ? index + 1 : /* @__PURE__ */ jsx12("span", { className: "w-2 h-2 rounded-full bg-current" }) }),
                        /* @__PURE__ */ jsxs8("span", { className: "ui-wizard-step-copy flex flex-col items-start", children: [
                          /* @__PURE__ */ jsx12("span", { className: cn("ui-wizard-step-title", stepTitleVariants({ state })), children: step.title }),
                          step.description && /* @__PURE__ */ jsx12("span", { className: cn("ui-wizard-step-description", stepDescriptionVariants()), children: step.description })
                        ] })
                      ]
                    }
                  )
                },
                step.id
              );
            }) })
          ] }),
          /* @__PURE__ */ jsx12(
            "div",
            {
              className: cn("ui-wizard-content", contentVariants({ animating: animatingStep || "none" })),
              children
            }
          ),
          /* @__PURE__ */ jsxs8("footer", { className: cn("ui-wizard-footer", footerVariants()), children: [
            /* @__PURE__ */ jsx12("div", { children: /* @__PURE__ */ jsx12(
              Button,
              {
                variant: "ghost",
                onClick: handleCancel,
                disabled: loading,
                children: cancelLabel
              }
            ) }),
            /* @__PURE__ */ jsxs8("div", { className: "flex items-center gap-[var(--ui-space-3)]", children: [
              !isFirstStep && /* @__PURE__ */ jsx12(
                Button,
                {
                  variant: "secondary",
                  onClick: handlePrevious,
                  disabled: loading,
                  children: previousLabel
                }
              ),
              isLastStep ? /* @__PURE__ */ jsx12(
                Button,
                {
                  variant: "primary",
                  onClick: handleComplete,
                  disabled: !canProceed || loading,
                  loading,
                  children: completeLabel
                }
              ) : /* @__PURE__ */ jsx12(
                Button,
                {
                  variant: "primary",
                  onClick: handleNext,
                  disabled: !canProceed || loading,
                  children: nextLabel
                }
              )
            ] })
          ] })
        ]
      }
    ) });
    if (typeof document === "undefined") {
      return dialog;
    }
    return createPortal(dialog, document.body);
  }
);
Wizard.displayName = "Wizard";

// src/components/Card/Card.tsx
import React13 from "react";
import { jsx as jsx13 } from "react/jsx-runtime";
var Card = React13.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx13("div", { ref, className: cn("ui-card", className), ...props })
);
Card.displayName = "Card";
var CardHeader = React13.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx13("div", { ref, className: cn("ui-card-header", className), ...props })
);
CardHeader.displayName = "CardHeader";
var CardTitle = React13.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx13("h3", { ref, className: cn("ui-card-title", className), ...props })
);
CardTitle.displayName = "CardTitle";
var CardDescription = React13.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx13("p", { ref, className: cn("ui-card-description", className), ...props })
);
CardDescription.displayName = "CardDescription";
var CardContent = React13.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx13("div", { ref, className: cn("ui-card-content", className), ...props })
);
CardContent.displayName = "CardContent";
var CardFooter = React13.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx13("div", { ref, className: cn("ui-card-footer", className), ...props })
);
CardFooter.displayName = "CardFooter";

// src/components/Separator/Separator.tsx
import React14 from "react";
import * as SeparatorPrimitive from "@radix-ui/react-separator";
import { jsx as jsx14 } from "react/jsx-runtime";
var Separator2 = React14.forwardRef(({ className, orientation = "horizontal", decorative = true, ...props }, ref) => /* @__PURE__ */ jsx14(
  SeparatorPrimitive.Root,
  {
    ref,
    decorative,
    orientation,
    className: cn(
      "shrink-0 bg-border",
      orientation === "horizontal" ? "h-px w-full" : "h-full w-px",
      className
    ),
    ...props
  }
));
Separator2.displayName = SeparatorPrimitive.Root.displayName;

// src/components/SectionHeader/SectionHeader.tsx
import React15 from "react";
import { jsx as jsx15, jsxs as jsxs9 } from "react/jsx-runtime";
var SectionHeader = React15.forwardRef(
  ({ title, size = "md", className, ...props }, ref) => {
    const textSize = size === "sm" ? "text-sm" : "text-base";
    return /* @__PURE__ */ jsxs9("div", { ref, className: cn("flex items-center gap-3", className), ...props, children: [
      /* @__PURE__ */ jsx15("h3", { className: cn("font-display font-semibold text-foreground", textSize), children: title }),
      /* @__PURE__ */ jsx15(Separator2, { className: "flex-1" })
    ] });
  }
);
SectionHeader.displayName = "SectionHeader";

// src/components/Table/Table.tsx
import React16 from "react";
import { jsx as jsx16 } from "react/jsx-runtime";
var Table = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16("div", { className: "w-full overflow-auto", children: /* @__PURE__ */ jsx16("table", { ref, className: cn("w-full caption-bottom text-sm", className), ...props }) })
);
Table.displayName = "Table";
var TableHeader = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16("thead", { ref, className: cn("[&_tr]:border-b", className), ...props })
);
TableHeader.displayName = "TableHeader";
var TableBody = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16("tbody", { ref, className: cn("[&_tr:last-child]:border-0", className), ...props })
);
TableBody.displayName = "TableBody";
var TableFooter = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16("tfoot", { ref, className: cn("border-t bg-muted/50 font-medium [&>tr]:last:border-b-0", className), ...props })
);
TableFooter.displayName = "TableFooter";
var TableRow = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16("tr", { ref, className: cn("border-b transition-colors hover:bg-muted/50 data-[state=selected]:bg-muted", className), ...props })
);
TableRow.displayName = "TableRow";
var TableHead = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16(
    "th",
    {
      ref,
      className: cn("h-10 px-2 text-left align-middle font-medium text-muted-foreground [&:has([role=checkbox])]:pr-0", className),
      ...props
    }
  )
);
TableHead.displayName = "TableHead";
var TableCell = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16(
    "td",
    {
      ref,
      className: cn("p-2 align-middle [&:has([role=checkbox])]:pr-0", className),
      ...props
    }
  )
);
TableCell.displayName = "TableCell";
var TableCaption = React16.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ jsx16("caption", { ref, className: cn("mt-4 text-sm text-muted-foreground", className), ...props })
);
TableCaption.displayName = "TableCaption";

// src/components/Badge/Badge.tsx
import { cva as cva4 } from "class-variance-authority";
import { jsx as jsx17 } from "react/jsx-runtime";
var badgeVariants = cva4(
  "ui-badge",
  {
    variants: {
      variant: {
        default: "ui-badge-default",
        secondary: "ui-badge-secondary",
        destructive: "ui-badge-destructive",
        outline: "ui-badge-outline",
        success: "ui-badge-success",
        warning: "ui-badge-warning",
        info: "ui-badge-info"
      }
    },
    defaultVariants: {
      variant: "default"
    }
  }
);
function Badge({ className, variant, ...props }) {
  return /* @__PURE__ */ jsx17("div", { className: cn(badgeVariants({ variant }), className), ...props });
}

// src/components/Tabs/Tabs.tsx
import * as React17 from "react";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import { jsx as jsx18 } from "react/jsx-runtime";
var Tabs = TabsPrimitive.Root;
var TabsList = React17.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx18(
  TabsPrimitive.List,
  {
    ref,
    className: cn("ui-tabs-list", className),
    ...props
  }
));
TabsList.displayName = TabsPrimitive.List.displayName;
var TabsTrigger = React17.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx18(
  TabsPrimitive.Trigger,
  {
    ref,
    className: cn("ui-tabs-trigger", className),
    ...props
  }
));
TabsTrigger.displayName = TabsPrimitive.Trigger.displayName;
var TabsContent = React17.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx18(
  TabsPrimitive.Content,
  {
    ref,
    className: cn("ui-tabs-content", className),
    ...props
  }
));
TabsContent.displayName = TabsPrimitive.Content.displayName;

// src/components/EmptyState/EmptyState.tsx
import { jsx as jsx19, jsxs as jsxs10 } from "react/jsx-runtime";
function EmptyState({
  icon,
  title,
  description,
  action,
  className = ""
}) {
  return /* @__PURE__ */ jsxs10(
    "div",
    {
      className: `flex flex-col items-center justify-center py-16 text-center ${className}`,
      children: [
        icon && /* @__PURE__ */ jsx19("div", { className: "text-muted-foreground/50 mb-4", children: icon }),
        /* @__PURE__ */ jsx19("h3", { className: "text-lg font-medium text-foreground mb-1", children: title }),
        description && /* @__PURE__ */ jsx19("p", { className: "text-muted-foreground mb-4", children: description }),
        action
      ]
    }
  );
}
export {
  Alert,
  AlertDescription,
  AlertTitle,
  Badge,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  Checkbox,
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
  EmptyState,
  InfoBox,
  Input,
  Label,
  Modal,
  Progress,
  ProgressBar,
  SectionHeader,
  Select,
  SelectContent,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
  Separator2 as Separator,
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Wizard,
  alertVariants,
  badgeVariants,
  buttonVariants,
  cn,
  wizardVariants
};
