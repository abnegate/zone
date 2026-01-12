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
  // Base styles
  [
    "inline-flex items-center justify-center gap-[var(--ui-space-1-5)]",
    "font-medium leading-none whitespace-nowrap",
    "border-none rounded-[var(--ui-radius-md)] cursor-pointer",
    "transition-all duration-[var(--ui-duration-fast)] ease-out",
    "select-none",
    "disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none",
    "active:scale-[0.98]",
    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--ui-border-focus)] focus-visible:ring-offset-2"
  ],
  {
    variants: {
      variant: {
        primary: [
          "bg-gradient-to-b from-[var(--ui-accent-500)] to-[var(--ui-accent-600)]",
          "text-white",
          "shadow-[0_1px_2px_rgba(0,0,0,0.1),inset_0_1px_0_rgba(255,255,255,0.1),inset_0_-1px_0_rgba(0,0,0,0.15)]",
          "hover:from-[var(--ui-accent-400)] hover:to-[var(--ui-accent-500)]",
          "hover:shadow-[0_4px_12px_rgba(6,182,212,0.3),inset_0_1px_0_rgba(255,255,255,0.15),inset_0_-1px_0_rgba(0,0,0,0.15)]"
        ],
        secondary: [
          "bg-[var(--ui-bg-surface)]",
          "text-[var(--ui-text-primary)]",
          "border border-[var(--ui-border)]",
          "hover:bg-[var(--ui-bg-hover)]",
          "hover:border-[var(--ui-border-strong)]"
        ],
        danger: [
          "bg-gradient-to-b from-[var(--ui-error-500)] to-[var(--ui-error-600)]",
          "text-white",
          "shadow-[0_1px_2px_rgba(0,0,0,0.1),inset_0_1px_0_rgba(255,255,255,0.1),inset_0_-1px_0_rgba(0,0,0,0.15)]",
          "hover:from-[var(--ui-error-400)] hover:to-[var(--ui-error-500)]",
          "hover:shadow-[0_4px_12px_rgba(244,63,94,0.3),inset_0_1px_0_rgba(255,255,255,0.15),inset_0_-1px_0_rgba(0,0,0,0.15)]"
        ],
        ghost: [
          "bg-transparent",
          "text-[var(--ui-text-secondary)]",
          "hover:bg-[var(--ui-bg-hover)]",
          "hover:text-[var(--ui-text-primary)]"
        ],
        generate: [
          "bg-gradient-to-b from-[var(--ui-secondary-500)] to-[var(--ui-secondary-600)]",
          "text-white",
          "shadow-[0_1px_2px_rgba(0,0,0,0.1),inset_0_1px_0_rgba(255,255,255,0.1),inset_0_-1px_0_rgba(0,0,0,0.15)]",
          "hover:from-[var(--ui-secondary-400)] hover:to-[var(--ui-secondary-500)]",
          "hover:shadow-[0_4px_12px_rgba(16,185,129,0.3),inset_0_1px_0_rgba(255,255,255,0.15),inset_0_-1px_0_rgba(0,0,0,0.15)]"
        ]
      },
      size: {
        sm: "h-7 px-[var(--ui-space-3)] text-[var(--ui-text-sm)] rounded-[var(--ui-radius-sm)]",
        md: "h-[34px] px-[var(--ui-space-4)] text-[var(--ui-text-sm)]",
        lg: "h-10 px-[var(--ui-space-5)] text-[var(--ui-text-base)] rounded-[var(--ui-radius-lg)]"
      },
      tone: {
        default: "",
        success: "",
        warning: "",
        info: ""
      }
    },
    compoundVariants: [
      // Tone modifiers for secondary variant
      {
        variant: "secondary",
        tone: "success",
        className: "text-[var(--ui-success-500)] border-[var(--ui-success-500)] hover:bg-[var(--ui-success-muted)]"
      },
      {
        variant: "secondary",
        tone: "warning",
        className: "text-[var(--ui-warning-500)] border-[var(--ui-warning-500)] hover:bg-[var(--ui-warning-muted)]"
      },
      {
        variant: "secondary",
        tone: "info",
        className: "text-[var(--ui-info-500)] border-[var(--ui-info-500)] hover:bg-[var(--ui-info-muted)]"
      }
    ],
    defaultVariants: {
      variant: "primary",
      size: "md",
      tone: "default"
    }
  }
);
var Button = forwardRef(
  ({
    className,
    variant,
    size,
    tone,
    asChild = false,
    loading = false,
    disabled,
    children,
    type = "button",
    ...props
  }, ref) => {
    const Comp = asChild ? Slot : "button";
    return /* @__PURE__ */ jsxs(
      Comp,
      {
        className: cn(buttonVariants({ variant, size, tone, className })),
        ref,
        disabled: disabled || loading,
        type,
        ...props,
        children: [
          loading && /* @__PURE__ */ jsx(
            "span",
            {
              className: "inline-block w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin",
              "aria-hidden": "true"
            }
          ),
          children
        ]
      }
    );
  }
);
Button.displayName = "Button";

// src/components/Input/Input.tsx
import { forwardRef as forwardRef2 } from "react";
import { cva as cva2 } from "class-variance-authority";
import { jsx as jsx2, jsxs as jsxs2 } from "react/jsx-runtime";
var inputVariants = cva2(
  [
    "w-full",
    "px-[var(--ui-space-3)] py-[var(--ui-space-2)]",
    "bg-[var(--ui-bg-elevated)]",
    "text-[var(--ui-text-primary)] text-[var(--ui-text-sm)]",
    "border border-[var(--ui-border)] rounded-[var(--ui-radius-md)]",
    "placeholder:text-[var(--ui-text-muted)]",
    "transition-all duration-[var(--ui-duration-fast)] ease-out",
    "focus:outline-none focus:border-[var(--ui-border-focus)] focus:ring-2 focus:ring-[var(--ui-accent-muted)]",
    "disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-[var(--ui-bg-muted)]"
  ],
  {
    variants: {
      variant: {
        default: "",
        error: "border-[var(--ui-error-500)] focus:border-[var(--ui-error-500)] focus:ring-[var(--ui-error-muted)]"
      },
      size: {
        sm: "h-8 text-[var(--ui-text-xs)]",
        md: "h-10",
        lg: "h-12 text-[var(--ui-text-base)]"
      }
    },
    defaultVariants: {
      variant: "default",
      size: "md"
    }
  }
);
var labelVariants = cva2([
  "block",
  "mb-[var(--ui-space-1-5)]",
  "text-[var(--ui-text-sm)] font-medium",
  "text-[var(--ui-text-secondary)]"
]);
var helpTextVariants = cva2([
  "mt-[var(--ui-space-1)]",
  "text-[var(--ui-text-xs)]",
  "text-[var(--ui-text-muted)]"
]);
var errorTextVariants = cva2([
  "mt-[var(--ui-space-1)]",
  "text-[var(--ui-text-xs)]",
  "text-[var(--ui-error-500)]"
]);
var Input = forwardRef2(
  ({
    label,
    helpText,
    error,
    onGenerate,
    id,
    className,
    type = "text",
    variant,
    size,
    ...props
  }, ref) => {
    const inputId = id || label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    const inputVariant = error ? "error" : variant;
    const inputElement = /* @__PURE__ */ jsx2(
      "input",
      {
        ref,
        id: inputId,
        className: cn(inputVariants({ variant: inputVariant, size, className })),
        type,
        "aria-invalid": !!error,
        "aria-describedby": error ? `${inputId}-error` : helpText ? `${inputId}-help` : void 0,
        ...props
      }
    );
    return /* @__PURE__ */ jsxs2("div", { className: "flex flex-col", children: [
      /* @__PURE__ */ jsx2("label", { className: cn(labelVariants()), htmlFor: inputId, children: label }),
      onGenerate ? /* @__PURE__ */ jsxs2("div", { className: "flex gap-[var(--ui-space-2)]", children: [
        inputElement,
        /* @__PURE__ */ jsx2(Button, { variant: "generate", type: "button", onClick: onGenerate, children: "Generate" })
      ] }) : inputElement,
      error && /* @__PURE__ */ jsx2("p", { id: `${inputId}-error`, className: cn(errorTextVariants()), role: "alert", children: error }),
      helpText && !error && /* @__PURE__ */ jsx2("p", { id: `${inputId}-help`, className: cn(helpTextVariants()), children: helpText })
    ] });
  }
);
Input.displayName = "Input";

// src/components/Select/Select.tsx
import { forwardRef as forwardRef3 } from "react";
import { cva as cva3 } from "class-variance-authority";
import { jsx as jsx3, jsxs as jsxs3 } from "react/jsx-runtime";
var selectVariants = cva3(
  [
    "w-full appearance-none",
    "px-[var(--ui-space-3)] py-[var(--ui-space-2)] pr-[var(--ui-space-8)]",
    "bg-[var(--ui-bg-elevated)]",
    "text-[var(--ui-text-primary)] text-[var(--ui-text-sm)]",
    "border border-[var(--ui-border)] rounded-[var(--ui-radius-md)]",
    "transition-all duration-[var(--ui-duration-fast)] ease-out",
    "focus:outline-none focus:border-[var(--ui-border-focus)] focus:ring-2 focus:ring-[var(--ui-accent-muted)]",
    "disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-[var(--ui-bg-muted)]",
    // Custom dropdown arrow
    "bg-[length:16px_16px] bg-no-repeat bg-[right_var(--ui-space-2)_center]",
    `bg-[url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 24 24' stroke='%2364748b'%3E%3Cpath stroke-linecap='round' stroke-linejoin='round' stroke-width='2' d='M19 9l-7 7-7-7'%3E%3C/path%3E%3C/svg%3E")]`
  ],
  {
    variants: {
      variant: {
        default: "",
        error: "border-[var(--ui-error-500)] focus:border-[var(--ui-error-500)] focus:ring-[var(--ui-error-muted)]"
      },
      size: {
        sm: "h-8 text-[var(--ui-text-xs)]",
        md: "h-10",
        lg: "h-12 text-[var(--ui-text-base)]"
      }
    },
    defaultVariants: {
      variant: "default",
      size: "md"
    }
  }
);
var labelVariants2 = cva3([
  "block",
  "mb-[var(--ui-space-1-5)]",
  "text-[var(--ui-text-sm)] font-medium",
  "text-[var(--ui-text-secondary)]"
]);
var helpTextVariants2 = cva3([
  "mt-[var(--ui-space-1)]",
  "text-[var(--ui-text-xs)]",
  "text-[var(--ui-text-muted)]"
]);
var errorTextVariants2 = cva3([
  "mt-[var(--ui-space-1)]",
  "text-[var(--ui-text-xs)]",
  "text-[var(--ui-error-500)]"
]);
var Select = forwardRef3(
  ({
    label,
    options,
    helpText,
    error,
    id,
    className,
    variant,
    size,
    ...props
  }, ref) => {
    const selectId = id || label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    const selectVariant = error ? "error" : variant;
    return /* @__PURE__ */ jsxs3("div", { className: "flex flex-col", children: [
      /* @__PURE__ */ jsx3("label", { className: cn(labelVariants2()), htmlFor: selectId, children: label }),
      /* @__PURE__ */ jsx3(
        "select",
        {
          ref,
          id: selectId,
          className: cn(selectVariants({ variant: selectVariant, size, className })),
          "aria-invalid": !!error,
          "aria-describedby": error ? `${selectId}-error` : helpText ? `${selectId}-help` : void 0,
          ...props,
          children: options.map((option) => /* @__PURE__ */ jsx3("option", { value: option.value, children: option.label }, option.value))
        }
      ),
      error && /* @__PURE__ */ jsx3("p", { id: `${selectId}-error`, className: cn(errorTextVariants2()), role: "alert", children: error }),
      helpText && !error && /* @__PURE__ */ jsx3("p", { id: `${selectId}-help`, className: cn(helpTextVariants2()), children: helpText })
    ] });
  }
);
Select.displayName = "Select";

// src/components/Checkbox/Checkbox.tsx
import { forwardRef as forwardRef4 } from "react";
import { cva as cva4 } from "class-variance-authority";
import { jsx as jsx4, jsxs as jsxs4 } from "react/jsx-runtime";
var checkboxVariants = cva4(
  [
    "peer shrink-0",
    "w-[18px] h-[18px]",
    "appearance-none cursor-pointer",
    "bg-[var(--ui-bg-elevated)]",
    "border border-[var(--ui-border)] rounded-[var(--ui-radius-sm)]",
    "transition-all duration-[var(--ui-duration-fast)] ease-out",
    "focus:outline-none focus:ring-2 focus:ring-[var(--ui-accent-muted)] focus:ring-offset-1",
    "checked:bg-[var(--ui-accent-500)] checked:border-[var(--ui-accent-500)]",
    `checked:bg-[url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='3' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='20 6 9 17 4 12'%3E%3C/polyline%3E%3C/svg%3E")] checked:bg-center checked:bg-no-repeat checked:bg-[length:12px_12px]`,
    "disabled:opacity-50 disabled:cursor-not-allowed"
  ],
  {
    variants: {
      size: {
        sm: "w-4 h-4 checked:bg-[length:10px_10px]",
        md: "w-[18px] h-[18px]",
        lg: "w-5 h-5 checked:bg-[length:14px_14px]"
      }
    },
    defaultVariants: {
      size: "md"
    }
  }
);
var labelVariants3 = cva4([
  "flex items-center gap-[var(--ui-space-2)] cursor-pointer",
  "text-[var(--ui-text-sm)]",
  "text-[var(--ui-text-primary)]",
  "select-none"
]);
var helpTextVariants3 = cva4([
  "mt-[var(--ui-space-1)]",
  "ml-[calc(18px+var(--ui-space-2))]",
  "text-[var(--ui-text-xs)]",
  "text-[var(--ui-text-muted)]"
]);
var Checkbox = forwardRef4(
  ({ label, helpText, id, className, size, ...props }, ref) => {
    const checkboxId = id || label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    return /* @__PURE__ */ jsxs4("div", { className: "flex flex-col", children: [
      /* @__PURE__ */ jsxs4("label", { className: cn(labelVariants3()), htmlFor: checkboxId, children: [
        /* @__PURE__ */ jsx4(
          "input",
          {
            ref,
            type: "checkbox",
            id: checkboxId,
            className: cn(checkboxVariants({ size, className })),
            ...props
          }
        ),
        /* @__PURE__ */ jsx4("span", { children: label })
      ] }),
      helpText && /* @__PURE__ */ jsx4("p", { className: cn(helpTextVariants3()), children: helpText })
    ] });
  }
);
Checkbox.displayName = "Checkbox";

// src/components/Modal/Modal.tsx
import { forwardRef as forwardRef5, useEffect } from "react";
import { cva as cva5 } from "class-variance-authority";
import { jsx as jsx5, jsxs as jsxs5 } from "react/jsx-runtime";
var overlayVariants = cva5([
  "fixed inset-0 z-50",
  "flex items-center justify-center",
  "bg-[var(--ui-overlay-medium)]",
  "backdrop-blur-sm",
  "animate-in fade-in duration-200"
]);
var modalVariants = cva5(
  [
    "relative",
    "bg-[var(--ui-bg-elevated)]",
    "border border-[var(--ui-border)]",
    "rounded-[var(--ui-radius-xl)]",
    "shadow-[var(--ui-shadow-xl)]",
    "p-[var(--ui-space-6)]",
    "max-h-[85vh] overflow-auto",
    "animate-in zoom-in-95 fade-in duration-200"
  ],
  {
    variants: {
      size: {
        sm: "w-full max-w-sm",
        md: "w-full max-w-md",
        lg: "w-full max-w-lg",
        xl: "w-full max-w-xl",
        full: "w-full max-w-[90vw]"
      }
    },
    defaultVariants: {
      size: "md"
    }
  }
);
var titleVariants = cva5([
  "text-[var(--ui-text-lg)] font-semibold",
  "text-[var(--ui-text-primary)]",
  "mb-[var(--ui-space-4)]"
]);
var Modal = forwardRef5(
  ({ isOpen, onClose, title, children, className, size, ...props }, ref) => {
    useEffect(() => {
      const handleEscape = (e) => {
        if (e.key === "Escape" && onClose) {
          onClose();
        }
      };
      if (isOpen) {
        document.addEventListener("keydown", handleEscape);
        document.body.style.overflow = "hidden";
      }
      return () => {
        document.removeEventListener("keydown", handleEscape);
        document.body.style.overflow = "";
      };
    }, [isOpen, onClose]);
    if (!isOpen) return null;
    return /* @__PURE__ */ jsx5(
      "div",
      {
        className: cn(overlayVariants()),
        onClick: onClose,
        role: "dialog",
        "aria-modal": "true",
        "aria-labelledby": "modal-title",
        children: /* @__PURE__ */ jsxs5(
          "div",
          {
            ref,
            className: cn(modalVariants({ size, className })),
            onClick: (e) => e.stopPropagation(),
            ...props,
            children: [
              /* @__PURE__ */ jsx5("h3", { id: "modal-title", className: cn(titleVariants()), children: title }),
              children
            ]
          }
        )
      }
    );
  }
);
Modal.displayName = "Modal";

// src/components/InfoBox/InfoBox.tsx
import { forwardRef as forwardRef6 } from "react";
import { cva as cva6 } from "class-variance-authority";
import { jsx as jsx6 } from "react/jsx-runtime";
var infoBoxVariants = cva6(
  [
    "flex items-start gap-[var(--ui-space-3)]",
    "p-[var(--ui-space-4)]",
    "rounded-[var(--ui-radius-lg)]",
    "border",
    "text-[var(--ui-text-sm)]"
  ],
  {
    variants: {
      variant: {
        info: [
          "bg-[var(--ui-info-muted)]",
          "border-[var(--ui-info-500)]",
          "text-[var(--ui-info-600)]",
          "[&_a]:text-[var(--ui-info-600)] [&_a]:underline"
        ],
        warning: [
          "bg-[var(--ui-warning-muted)]",
          "border-[var(--ui-warning-500)]",
          "text-[var(--ui-warning-600)]",
          "[&_a]:text-[var(--ui-warning-600)] [&_a]:underline"
        ],
        success: [
          "bg-[var(--ui-success-muted)]",
          "border-[var(--ui-success-500)]",
          "text-[var(--ui-success-600)]",
          "[&_a]:text-[var(--ui-success-600)] [&_a]:underline"
        ],
        error: [
          "bg-[var(--ui-error-muted)]",
          "border-[var(--ui-error-500)]",
          "text-[var(--ui-error-600)]",
          "[&_a]:text-[var(--ui-error-600)] [&_a]:underline"
        ]
      },
      size: {
        sm: "p-[var(--ui-space-3)] text-[var(--ui-text-xs)]",
        md: "p-[var(--ui-space-4)]",
        lg: "p-[var(--ui-space-5)] text-[var(--ui-text-base)]"
      }
    },
    defaultVariants: {
      variant: "info",
      size: "md"
    }
  }
);
var InfoBox = forwardRef6(
  ({ variant, size, children, className, ...props }, ref) => {
    return /* @__PURE__ */ jsx6(
      "div",
      {
        ref,
        role: "alert",
        className: cn(infoBoxVariants({ variant, size, className })),
        ...props,
        children
      }
    );
  }
);
InfoBox.displayName = "InfoBox";

// src/components/ProgressBar/ProgressBar.tsx
import { forwardRef as forwardRef7 } from "react";
import { cva as cva7 } from "class-variance-authority";
import { jsx as jsx7, jsxs as jsxs6 } from "react/jsx-runtime";
var progressBarVariants = cva7(
  ["flex flex-col gap-[var(--ui-space-1-5)]"],
  {
    variants: {
      size: {
        sm: "",
        md: "",
        lg: ""
      }
    },
    defaultVariants: {
      size: "md"
    }
  }
);
var trackVariants = cva7(
  [
    "w-full overflow-hidden",
    "bg-[var(--ui-bg-muted)]",
    "rounded-full"
  ],
  {
    variants: {
      size: {
        sm: "h-1",
        md: "h-2",
        lg: "h-3"
      }
    },
    defaultVariants: {
      size: "md"
    }
  }
);
var fillVariants = cva7(
  [
    "h-full",
    "bg-gradient-to-r from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]",
    "rounded-full",
    "transition-all duration-[var(--ui-duration-normal)] ease-out"
  ],
  {
    variants: {
      variant: {
        default: "from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]",
        success: "from-[var(--ui-success-500)] to-[var(--ui-success-400)]",
        warning: "from-[var(--ui-warning-500)] to-[var(--ui-warning-400)]",
        error: "from-[var(--ui-error-500)] to-[var(--ui-error-400)]"
      }
    },
    defaultVariants: {
      variant: "default"
    }
  }
);
var headerVariants = cva7([
  "flex items-center justify-between",
  "text-[var(--ui-text-sm)]",
  "text-[var(--ui-text-secondary)]"
]);
var ProgressBar = forwardRef7(
  ({
    value,
    max = 100,
    label,
    showPercentage = true,
    className,
    size,
    variant,
    ...props
  }, ref) => {
    const percentage = Math.min(100, Math.max(0, Math.round(value / max * 100)));
    return /* @__PURE__ */ jsxs6(
      "div",
      {
        ref,
        className: cn(progressBarVariants({ size, className })),
        role: "progressbar",
        "aria-valuenow": value,
        "aria-valuemin": 0,
        "aria-valuemax": max,
        "aria-label": label,
        ...props,
        children: [
          (label || showPercentage) && /* @__PURE__ */ jsxs6("div", { className: cn(headerVariants()), children: [
            /* @__PURE__ */ jsx7("span", { children: label }),
            showPercentage && /* @__PURE__ */ jsxs6("span", { children: [
              percentage,
              "%"
            ] })
          ] }),
          /* @__PURE__ */ jsx7("div", { className: cn(trackVariants({ size })), children: /* @__PURE__ */ jsx7(
            "div",
            {
              className: cn(fillVariants({ variant })),
              style: { width: `${percentage}%` }
            }
          ) })
        ]
      }
    );
  }
);
ProgressBar.displayName = "ProgressBar";

// src/components/Wizard/Wizard.tsx
import { forwardRef as forwardRef8, useEffect as useEffect2, useCallback, useState } from "react";
import { cva as cva8 } from "class-variance-authority";
import { jsx as jsx8, jsxs as jsxs7 } from "react/jsx-runtime";
var overlayVariants2 = cva8([
  "fixed inset-0 z-50",
  "flex items-center justify-center",
  "bg-[var(--ui-overlay-medium)]",
  "backdrop-blur-sm",
  "animate-in fade-in duration-200"
]);
var wizardVariants = cva8(
  [
    "relative flex flex-col",
    "bg-[var(--ui-bg-elevated)]",
    "border border-[var(--ui-border)]",
    "rounded-[var(--ui-radius-xl)]",
    "shadow-[var(--ui-shadow-xl)]",
    "max-h-[90vh] overflow-hidden",
    "animate-in zoom-in-95 fade-in duration-200"
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
var headerVariants2 = cva8([
  "flex items-start justify-between gap-[var(--ui-space-4)]",
  "p-[var(--ui-space-6)]",
  "border-b border-[var(--ui-border)]"
]);
var titleVariants2 = cva8([
  "text-[var(--ui-text-xl)] font-semibold",
  "text-[var(--ui-text-primary)]"
]);
var subtitleVariants = cva8([
  "mt-[var(--ui-space-1)]",
  "text-[var(--ui-text-sm)]",
  "text-[var(--ui-text-muted)]"
]);
var closeButtonVariants = cva8([
  "flex items-center justify-center",
  "w-8 h-8",
  "rounded-[var(--ui-radius-md)]",
  "text-[var(--ui-text-muted)]",
  "hover:bg-[var(--ui-bg-hover)] hover:text-[var(--ui-text-primary)]",
  "transition-colors duration-[var(--ui-duration-fast)]",
  "disabled:opacity-50 disabled:cursor-not-allowed"
]);
var stepsNavVariants = cva8([
  "px-[var(--ui-space-6)] py-[var(--ui-space-4)]",
  "border-b border-[var(--ui-border)]",
  "bg-[var(--ui-bg-surface)]"
]);
var progressTrackVariants = cva8([
  "h-1 w-full",
  "bg-[var(--ui-bg-muted)]",
  "rounded-full",
  "mb-[var(--ui-space-4)]",
  "overflow-hidden"
]);
var progressFillVariants = cva8([
  "h-full",
  "bg-gradient-to-r from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]",
  "rounded-full",
  "transition-all duration-[var(--ui-duration-normal)] ease-out"
]);
var stepListVariants = cva8([
  "flex items-center justify-between gap-[var(--ui-space-2)]",
  "list-none m-0 p-0"
]);
var stepItemVariants = cva8(
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
var stepButtonVariants = cva8(
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
var stepIndicatorVariants = cva8(
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
var stepTitleVariants = cva8(
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
var stepDescriptionVariants = cva8([
  "text-[var(--ui-text-xs)]",
  "text-[var(--ui-text-muted)]"
]);
var contentVariants = cva8(
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
var footerVariants = cva8([
  "flex items-center justify-between",
  "p-[var(--ui-space-6)]",
  "border-t border-[var(--ui-border)]",
  "bg-[var(--ui-bg-surface)]"
]);
var Wizard = forwardRef8(
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
    useEffect2(() => {
      const handleEscape = (e) => {
        if (e.key === "Escape" && onClose) {
          onClose();
        }
      };
      if (isOpen) {
        document.addEventListener("keydown", handleEscape);
        document.body.style.overflow = "hidden";
      }
      return () => {
        document.removeEventListener("keydown", handleEscape);
        document.body.style.overflow = "";
      };
    }, [isOpen, onClose]);
    const handleNext = useCallback(() => {
      if (currentStep < steps.length - 1 && canProceed && !loading) {
        setAnimatingStep("next");
        setTimeout(() => {
          onStepChange?.(currentStep + 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, steps.length, canProceed, loading, onStepChange]);
    const handlePrevious = useCallback(() => {
      if (currentStep > 0 && !loading) {
        setAnimatingStep("prev");
        setTimeout(() => {
          onStepChange?.(currentStep - 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, loading, onStepChange]);
    const handleStepClick = useCallback(
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
    const handleComplete = useCallback(() => {
      if (canProceed && !loading) {
        onComplete?.();
      }
    }, [canProceed, loading, onComplete]);
    const handleCancel = useCallback(() => {
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
    return /* @__PURE__ */ jsx8("div", { className: cn(overlayVariants2()), onClick: onClose, children: /* @__PURE__ */ jsxs7(
      "div",
      {
        ref,
        className: cn(wizardVariants({ size, className })),
        onClick: (e) => e.stopPropagation(),
        role: "dialog",
        "aria-modal": "true",
        "aria-labelledby": "wizard-title",
        ...props,
        children: [
          /* @__PURE__ */ jsxs7("header", { className: cn(headerVariants2()), children: [
            /* @__PURE__ */ jsxs7("div", { children: [
              /* @__PURE__ */ jsx8("h2", { id: "wizard-title", className: cn(titleVariants2()), children: title }),
              subtitle && /* @__PURE__ */ jsx8("p", { className: cn(subtitleVariants()), children: subtitle })
            ] }),
            onClose && /* @__PURE__ */ jsx8(
              "button",
              {
                type: "button",
                className: cn(closeButtonVariants()),
                onClick: onClose,
                "aria-label": "Close wizard",
                disabled: loading,
                children: /* @__PURE__ */ jsx8(
                  "svg",
                  {
                    className: "w-5 h-5",
                    viewBox: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    strokeWidth: "2",
                    strokeLinecap: "round",
                    strokeLinejoin: "round",
                    children: /* @__PURE__ */ jsx8("path", { d: "M18 6L6 18M6 6l12 12" })
                  }
                )
              }
            )
          ] }),
          /* @__PURE__ */ jsxs7("nav", { className: cn(stepsNavVariants()), "aria-label": "Wizard steps", children: [
            /* @__PURE__ */ jsx8("div", { className: cn(progressTrackVariants()), children: /* @__PURE__ */ jsx8(
              "div",
              {
                className: cn(progressFillVariants()),
                style: { width: `${progressPercent}%` }
              }
            ) }),
            /* @__PURE__ */ jsx8("ol", { className: cn(stepListVariants()), children: steps.map((step, index) => {
              const state = getStepState(index);
              const isClickable = allowStepClick && (state === "completed" || canProceed && index === currentStep + 1);
              return /* @__PURE__ */ jsx8(
                "li",
                {
                  className: cn(stepItemVariants({ state, clickable: isClickable })),
                  children: /* @__PURE__ */ jsxs7(
                    "button",
                    {
                      type: "button",
                      className: cn(stepButtonVariants({ state, clickable: isClickable })),
                      onClick: () => handleStepClick(index),
                      disabled: !isClickable || loading,
                      "aria-current": state === "current" ? "step" : void 0,
                      children: [
                        /* @__PURE__ */ jsx8("span", { className: cn(stepIndicatorVariants({ state })), children: state === "completed" ? /* @__PURE__ */ jsx8(
                          "svg",
                          {
                            className: "w-4 h-4",
                            viewBox: "0 0 24 24",
                            fill: "none",
                            stroke: "currentColor",
                            strokeWidth: "3",
                            strokeLinecap: "round",
                            strokeLinejoin: "round",
                            children: /* @__PURE__ */ jsx8("polyline", { points: "20 6 9 17 4 12" })
                          }
                        ) : step.icon ? step.icon : showStepNumbers ? index + 1 : /* @__PURE__ */ jsx8("span", { className: "w-2 h-2 rounded-full bg-current" }) }),
                        /* @__PURE__ */ jsxs7("span", { className: "flex flex-col items-start", children: [
                          /* @__PURE__ */ jsx8("span", { className: cn(stepTitleVariants({ state })), children: step.title }),
                          step.description && /* @__PURE__ */ jsx8("span", { className: cn(stepDescriptionVariants()), children: step.description })
                        ] })
                      ]
                    }
                  )
                },
                step.id
              );
            }) })
          ] }),
          /* @__PURE__ */ jsx8(
            "div",
            {
              className: cn(contentVariants({ animating: animatingStep || "none" })),
              children
            }
          ),
          /* @__PURE__ */ jsxs7("footer", { className: cn(footerVariants()), children: [
            /* @__PURE__ */ jsx8("div", { children: /* @__PURE__ */ jsx8(
              Button,
              {
                variant: "ghost",
                onClick: handleCancel,
                disabled: loading,
                children: cancelLabel
              }
            ) }),
            /* @__PURE__ */ jsxs7("div", { className: "flex items-center gap-[var(--ui-space-3)]", children: [
              !isFirstStep && /* @__PURE__ */ jsx8(
                Button,
                {
                  variant: "secondary",
                  onClick: handlePrevious,
                  disabled: loading,
                  children: previousLabel
                }
              ),
              isLastStep ? /* @__PURE__ */ jsx8(
                Button,
                {
                  variant: "primary",
                  onClick: handleComplete,
                  disabled: !canProceed || loading,
                  loading,
                  children: completeLabel
                }
              ) : /* @__PURE__ */ jsx8(
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
  }
);
Wizard.displayName = "Wizard";
export {
  Button,
  Checkbox,
  InfoBox,
  Input,
  Modal,
  ProgressBar,
  Select,
  Wizard,
  buttonVariants,
  checkboxVariants,
  cn,
  fillVariants,
  infoBoxVariants,
  inputVariants,
  modalVariants,
  progressBarVariants,
  selectVariants,
  trackVariants,
  wizardVariants
};
