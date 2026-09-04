"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/index.ts
var index_exports = {};
__export(index_exports, {
  Alert: () => Alert,
  AlertDescription: () => AlertDescription,
  AlertTitle: () => AlertTitle,
  Badge: () => Badge,
  Button: () => Button,
  Card: () => Card,
  CardContent: () => CardContent,
  CardDescription: () => CardDescription,
  CardFooter: () => CardFooter,
  CardHeader: () => CardHeader,
  CardTitle: () => CardTitle,
  Checkbox: () => Checkbox,
  Dialog: () => Dialog,
  DialogClose: () => DialogClose,
  DialogContent: () => DialogContent,
  DialogDescription: () => DialogDescription,
  DialogFooter: () => DialogFooter,
  DialogHeader: () => DialogHeader,
  DialogOverlay: () => DialogOverlay,
  DialogPortal: () => DialogPortal,
  DialogTitle: () => DialogTitle,
  DialogTrigger: () => DialogTrigger,
  EmptyState: () => EmptyState,
  InfoBox: () => InfoBox,
  Input: () => Input,
  Label: () => Label,
  Modal: () => Modal,
  Progress: () => Progress,
  ProgressBar: () => ProgressBar,
  SectionHeader: () => SectionHeader,
  Select: () => Select,
  SelectContent: () => SelectContent,
  SelectItem: () => SelectItem,
  SelectLabel: () => SelectLabel,
  SelectSeparator: () => SelectSeparator,
  SelectTrigger: () => SelectTrigger,
  SelectValue: () => SelectValue,
  Separator: () => Separator2,
  Table: () => Table,
  TableBody: () => TableBody,
  TableCaption: () => TableCaption,
  TableCell: () => TableCell,
  TableFooter: () => TableFooter,
  TableHead: () => TableHead,
  TableHeader: () => TableHeader,
  TableRow: () => TableRow,
  Tabs: () => Tabs,
  TabsContent: () => TabsContent,
  TabsList: () => TabsList,
  TabsTrigger: () => TabsTrigger,
  Wizard: () => Wizard,
  alertVariants: () => alertVariants,
  badgeVariants: () => badgeVariants,
  buttonVariants: () => buttonVariants,
  cn: () => cn,
  wizardVariants: () => wizardVariants
});
module.exports = __toCommonJS(index_exports);

// src/components/Button/Button.tsx
var import_react = require("react");
var import_react_slot = require("@radix-ui/react-slot");
var import_class_variance_authority = require("class-variance-authority");

// src/lib/utils.ts
var import_clsx = require("clsx");
var import_tailwind_merge = require("tailwind-merge");
function cn(...inputs) {
  return (0, import_tailwind_merge.twMerge)((0, import_clsx.clsx)(inputs));
}

// src/components/Button/Button.tsx
var import_jsx_runtime = require("react/jsx-runtime");
var buttonVariants = (0, import_class_variance_authority.cva)(
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
var Button = (0, import_react.forwardRef)(
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
    const Comp = asChild ? import_react_slot.Slot : "button";
    const resolvedVariant = LEGACY_VARIANT_MAP[variant ?? ""] ?? variant;
    const resolvedSize = LEGACY_SIZE_MAP[size ?? ""] ?? size;
    return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
      Comp,
      {
        ref,
        className: cn(buttonVariants({ variant: resolvedVariant, size: resolvedSize, className })),
        disabled: disabled || loading,
        type: asChild ? void 0 : type,
        ...props,
        children: [
          loading && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "ui-btn-spinner", "aria-hidden": "true" }),
          children
        ]
      }
    );
  }
);
Button.displayName = "Button";

// src/components/Input/Input.tsx
var import_react3 = require("react");

// src/components/Label/Label.tsx
var import_react2 = __toESM(require("react"));
var LabelPrimitive = __toESM(require("@radix-ui/react-label"));
var import_jsx_runtime2 = require("react/jsx-runtime");
var Label = import_react2.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(LabelPrimitive.Root, { ref, className: cn("ui-label", className), ...props }));
Label.displayName = LabelPrimitive.Root.displayName;

// src/components/Input/Input.tsx
var import_jsx_runtime3 = require("react/jsx-runtime");
var Input = (0, import_react3.forwardRef)(
  ({ label, helpText, error, onGenerate, id, className, type = "text", ...props }, ref) => {
    const inputId = id || (label ? label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") : void 0);
    const describedBy = error ? `${inputId}-error` : helpText ? `${inputId}-help` : void 0;
    const inputElement = /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("div", { className: "ui-input-wrapper", children: [
      label && /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(Label, { htmlFor: inputId, children: label }),
      onGenerate ? /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("div", { className: "ui-input-with-button", children: [
        inputElement,
        /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(Button, { variant: "secondary", type: "button", onClick: onGenerate, children: "Generate" })
      ] }) : inputElement,
      error && /* @__PURE__ */ (0, import_jsx_runtime3.jsx)("p", { id: `${inputId}-error`, className: "ui-input-error-text", role: "alert", children: error }),
      helpText && !error && /* @__PURE__ */ (0, import_jsx_runtime3.jsx)("p", { id: `${inputId}-help`, className: "ui-input-help-text", children: helpText })
    ] });
  }
);
Input.displayName = "Input";

// src/components/Select/Select.tsx
var import_react4 = require("react");
var SelectPrimitive = __toESM(require("@radix-ui/react-select"));
var import_jsx_runtime4 = require("react/jsx-runtime");
var SelectTrigger = (0, import_react4.forwardRef)(({ className, children, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime4.jsxs)(
  SelectPrimitive.Trigger,
  {
    ref,
    className: cn("ui-select-trigger", className),
    ...props,
    children: [
      children,
      /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectPrimitive.Icon, { asChild: true, children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "2",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-icon",
          children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("polyline", { points: "6 9 12 15 18 9" })
        }
      ) })
    ]
  }
));
SelectTrigger.displayName = SelectPrimitive.Trigger.displayName;
var SelectContent = (0, import_react4.forwardRef)(({ className, children, position = "popper", ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectPrimitive.Portal, { children: /* @__PURE__ */ (0, import_jsx_runtime4.jsxs)(
  SelectPrimitive.Content,
  {
    ref,
    className: cn("ui-select-content", className),
    position,
    ...props,
    children: [
      /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectPrimitive.ScrollUpButton, { className: "ui-select-scroll-button", children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "2",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-scroll-icon",
          children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("polyline", { points: "18 15 12 9 6 15" })
        }
      ) }),
      /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
        SelectPrimitive.Viewport,
        {
          className: cn("ui-select-viewport", position === "popper" && "ui-select-viewport-popper"),
          children
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectPrimitive.ScrollDownButton, { className: "ui-select-scroll-button", children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "2",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-scroll-icon",
          children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("polyline", { points: "6 9 12 15 18 9" })
        }
      ) })
    ]
  }
) }));
SelectContent.displayName = SelectPrimitive.Content.displayName;
var SelectLabel = (0, import_react4.forwardRef)(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
  SelectPrimitive.Label,
  {
    ref,
    className: cn("ui-select-label", className),
    ...props
  }
));
SelectLabel.displayName = SelectPrimitive.Label.displayName;
var SelectItem = (0, import_react4.forwardRef)(({ className, children, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime4.jsxs)(
  SelectPrimitive.Item,
  {
    ref,
    className: cn("ui-select-item", className),
    ...props,
    children: [
      /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("span", { className: "ui-select-item-indicator", children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectPrimitive.ItemIndicator, { children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
        "svg",
        {
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "3",
          strokeLinecap: "round",
          strokeLinejoin: "round",
          className: "ui-select-item-icon",
          children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("polyline", { points: "20 6 9 17 4 12" })
        }
      ) }) }),
      /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectPrimitive.ItemText, { children })
    ]
  }
));
SelectItem.displayName = SelectPrimitive.Item.displayName;
var SelectSeparator = (0, import_react4.forwardRef)(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
  SelectPrimitive.Separator,
  {
    ref,
    className: cn("ui-select-separator", className),
    ...props
  }
));
SelectSeparator.displayName = SelectPrimitive.Separator.displayName;
var SelectValue = SelectPrimitive.Value;
var Select = (0, import_react4.forwardRef)(
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
    const handleValueChange = (0, import_react4.useCallback)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime4.jsxs)("div", { className: "ui-select-wrapper", children: [
      label && /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(Label, { htmlFor: selectId, children: label }),
      /* @__PURE__ */ (0, import_jsx_runtime4.jsxs)(
        SelectPrimitive.Root,
        {
          value,
          defaultValue,
          onValueChange: handleValueChange,
          name,
          disabled,
          required,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
              SelectTrigger,
              {
                id: selectId,
                ref,
                className: cn(error && "ui-select-trigger-error", className),
                children: /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectValue, { placeholder: placeholderText })
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectContent, { children: options.filter((option) => option.value !== "").map((option) => /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(SelectItem, { value: option.value, disabled: option.disabled, children: option.label }, option.value)) })
          ]
        }
      ),
      error && /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("p", { className: "ui-select-error-text", children: error }),
      helpText && !error && /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("p", { className: "ui-select-help-text", children: helpText })
    ] });
  }
);
Select.displayName = "Select";

// src/components/Checkbox/Checkbox.tsx
var import_react5 = require("react");
var CheckboxPrimitive = __toESM(require("@radix-ui/react-checkbox"));
var import_jsx_runtime5 = require("react/jsx-runtime");
var Checkbox = (0, import_react5.forwardRef)(
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
    const handleCheckedChange = (0, import_react5.useCallback)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "ui-checkbox-wrapper", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
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
          "aria-describedby": helpText && checkboxId ? `${checkboxId}-help` : void 0,
          ...props,
          children: /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(CheckboxPrimitive.Indicator, { className: "ui-checkbox-indicator", children: /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
            "svg",
            {
              viewBox: "0 0 24 24",
              fill: "none",
              stroke: "currentColor",
              strokeWidth: "3",
              strokeLinecap: "round",
              strokeLinejoin: "round",
              className: "ui-checkbox-icon",
              children: /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("polyline", { points: "20 6 9 17 4 12" })
            }
          ) })
        }
      ),
      (label || helpText) && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "ui-checkbox-copy", children: [
        label && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(Label, { htmlFor: checkboxId, children: label }),
        helpText && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("p", { id: checkboxId ? `${checkboxId}-help` : void 0, className: "ui-checkbox-help-text", children: helpText })
      ] })
    ] });
  }
);
Checkbox.displayName = "Checkbox";

// src/components/Modal/Modal.tsx
var import_react7 = __toESM(require("react"));

// src/components/Dialog/Dialog.tsx
var import_react6 = __toESM(require("react"));
var DialogPrimitive = __toESM(require("@radix-ui/react-dialog"));
var import_jsx_runtime6 = require("react/jsx-runtime");
var Dialog = DialogPrimitive.Root;
var DialogTrigger = DialogPrimitive.Trigger;
var DialogPortal = DialogPrimitive.Portal;
var DialogClose = DialogPrimitive.Close;
var DialogOverlay = import_react6.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
  DialogPrimitive.Overlay,
  {
    ref,
    className: cn("ui-dialog-overlay", className),
    ...props
  }
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;
var DialogContent = import_react6.default.forwardRef(({ className, children, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(DialogPortal, { children: [
  /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(DialogOverlay, {}),
  /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(
    DialogPrimitive.Content,
    {
      ref,
      className: cn("ui-dialog-content", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(DialogPrimitive.Close, { className: "ui-dialog-close", children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
            "svg",
            {
              viewBox: "0 0 24 24",
              fill: "none",
              stroke: "currentColor",
              strokeWidth: "2",
              strokeLinecap: "round",
              strokeLinejoin: "round",
              className: "ui-dialog-close-icon",
              children: /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("path", { d: "M18 6L6 18M6 6l12 12" })
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "sr-only", children: "Close" })
        ] })
      ]
    }
  )
] }));
DialogContent.displayName = DialogPrimitive.Content.displayName;
var DialogHeader = ({ className, ...props }) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: cn("ui-dialog-header", className), ...props });
var DialogFooter = ({ className, ...props }) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: cn("ui-dialog-footer", className), ...props });
var DialogTitle = import_react6.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
  DialogPrimitive.Title,
  {
    ref,
    className: cn("ui-dialog-title", className),
    ...props
  }
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;
var DialogDescription = import_react6.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
  DialogPrimitive.Description,
  {
    ref,
    className: cn("ui-dialog-description", className),
    ...props
  }
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

// src/components/Modal/Modal.tsx
var import_jsx_runtime7 = require("react/jsx-runtime");
var SIZE_CLASS_MAP = {
  sm: "max-w-sm",
  md: "max-w-md",
  lg: "max-w-lg",
  xl: "max-w-xl",
  full: "max-w-[90vw]"
};
var Modal = import_react7.default.forwardRef(
  ({ isOpen, onClose, title, size = "md", children, className, ...props }, ref) => {
    return /* @__PURE__ */ (0, import_jsx_runtime7.jsx)(Dialog, { open: isOpen, onOpenChange: (open) => !open ? onClose?.() : void 0, children: /* @__PURE__ */ (0, import_jsx_runtime7.jsxs)(DialogContent, { ref, className: cn(SIZE_CLASS_MAP[size], className), ...props, children: [
      /* @__PURE__ */ (0, import_jsx_runtime7.jsx)(DialogHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime7.jsx)(DialogTitle, { children: title }) }),
      children
    ] }) });
  }
);
Modal.displayName = "Modal";

// src/components/InfoBox/InfoBox.tsx
var import_react9 = __toESM(require("react"));

// src/components/Alert/Alert.tsx
var import_react8 = __toESM(require("react"));
var import_class_variance_authority2 = require("class-variance-authority");
var import_jsx_runtime8 = require("react/jsx-runtime");
var alertVariants = (0, import_class_variance_authority2.cva)(
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
var Alert = import_react8.default.forwardRef(({ className, variant, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime8.jsx)("div", { ref, role: "alert", className: cn(alertVariants({ variant }), className), ...props }));
Alert.displayName = "Alert";
var AlertTitle = import_react8.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime8.jsx)("h5", { ref, className: cn("ui-alert-title", className), ...props })
);
AlertTitle.displayName = "AlertTitle";
var AlertDescription = import_react8.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime8.jsx)("div", { ref, className: cn("ui-alert-description", className), ...props }));
AlertDescription.displayName = "AlertDescription";

// src/components/InfoBox/InfoBox.tsx
var import_jsx_runtime9 = require("react/jsx-runtime");
var INFOBOX_VARIANT_MAP = {
  default: "default",
  info: "default",
  success: "default",
  warning: "destructive",
  error: "destructive",
  destructive: "destructive"
};
var InfoBox = import_react9.default.forwardRef(
  ({ variant = "default", className, ...props }, ref) => {
    const mappedVariant = INFOBOX_VARIANT_MAP[variant] ?? "default";
    return /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(Alert, { ref, variant: mappedVariant, className, ...props });
  }
);
InfoBox.displayName = "InfoBox";

// src/components/ProgressBar/ProgressBar.tsx
var import_react11 = __toESM(require("react"));

// src/components/Progress/Progress.tsx
var import_react10 = __toESM(require("react"));
var ProgressPrimitive = __toESM(require("@radix-ui/react-progress"));
var import_jsx_runtime10 = require("react/jsx-runtime");
var Progress = import_react10.default.forwardRef(({ className, value = 0, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime10.jsx)(
  ProgressPrimitive.Root,
  {
    ref,
    className: cn("relative h-2 w-full overflow-hidden rounded-full bg-secondary", className),
    ...props,
    children: /* @__PURE__ */ (0, import_jsx_runtime10.jsx)(
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
var import_jsx_runtime11 = require("react/jsx-runtime");
var ProgressBar = import_react11.default.forwardRef(
  ({ value, max = 100, label, showPercentage = true, className, ...props }, ref) => {
    const percentage = Math.min(100, Math.max(0, Math.round(value / max * 100)));
    return /* @__PURE__ */ (0, import_jsx_runtime11.jsxs)("div", { ref, className: cn("grid gap-2", className), ...props, children: [
      (label || showPercentage) && /* @__PURE__ */ (0, import_jsx_runtime11.jsxs)("div", { className: "flex items-center justify-between text-sm text-muted-foreground", children: [
        /* @__PURE__ */ (0, import_jsx_runtime11.jsx)("span", { children: label }),
        showPercentage && /* @__PURE__ */ (0, import_jsx_runtime11.jsxs)("span", { children: [
          percentage,
          "%"
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime11.jsx)(Progress, { value: percentage, "aria-label": label || "Progress" })
    ] });
  }
);
ProgressBar.displayName = "ProgressBar";

// src/components/Wizard/Wizard.tsx
var import_react12 = require("react");
var import_react_dom = require("react-dom");
var import_class_variance_authority3 = require("class-variance-authority");
var import_jsx_runtime12 = require("react/jsx-runtime");
var overlayVariants = (0, import_class_variance_authority3.cva)([
  "fixed inset-0 z-50",
  "flex items-center justify-center",
  "bg-[var(--ui-overlay-medium)]",
  "backdrop-blur-sm"
]);
var wizardVariants = (0, import_class_variance_authority3.cva)(
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
        sm: "ui-wizard--sm",
        md: "ui-wizard--md",
        lg: "ui-wizard--lg",
        xl: "ui-wizard--xl"
      }
    },
    defaultVariants: {
      size: "md"
    }
  }
);
var headerVariants = (0, import_class_variance_authority3.cva)([
  "flex items-start justify-between gap-[var(--ui-space-4)]",
  "p-[var(--ui-panel-padding)]",
  "border-b border-[var(--ui-border)]"
]);
var titleVariants = (0, import_class_variance_authority3.cva)([
  "text-[var(--ui-heading-size)] font-display font-semibold leading-tight tracking-tight",
  "text-[var(--ui-text-primary)]"
]);
var subtitleVariants = (0, import_class_variance_authority3.cva)([
  "mt-[var(--ui-space-1)]",
  "text-[var(--ui-text-sm)]",
  "text-[var(--ui-text-muted)]"
]);
var closeButtonVariants = (0, import_class_variance_authority3.cva)([
  "flex items-center justify-center",
  "w-8 h-8",
  "rounded-[var(--ui-radius-md)]",
  "text-[var(--ui-text-muted)]",
  "hover:bg-[var(--ui-bg-hover)] hover:text-[var(--ui-text-primary)]",
  "transition-colors duration-[var(--ui-duration-fast)]",
  "disabled:opacity-50 disabled:cursor-not-allowed"
]);
var stepsNavVariants = (0, import_class_variance_authority3.cva)([
  "px-[var(--ui-panel-padding)] py-[var(--ui-space-3)]",
  "border-b border-[var(--ui-border)]",
  "bg-[var(--ui-bg-surface)]"
]);
var progressTrackVariants = (0, import_class_variance_authority3.cva)([
  "h-1 w-full",
  "bg-[var(--ui-bg-muted)]",
  "rounded-full",
  "mb-[var(--ui-space-4)]",
  "overflow-hidden"
]);
var progressFillVariants = (0, import_class_variance_authority3.cva)([
  "h-full",
  "bg-gradient-to-r from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]",
  "rounded-full",
  "transition-all duration-[var(--ui-duration-normal)] ease-out"
]);
var stepListVariants = (0, import_class_variance_authority3.cva)([
  "flex items-center justify-between gap-[var(--ui-space-2)]",
  "list-none m-0 p-0"
]);
var stepItemVariants = (0, import_class_variance_authority3.cva)(["flex-1"], {
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
});
var stepButtonVariants = (0, import_class_variance_authority3.cva)(
  [
    "flex items-center gap-[var(--ui-space-2)] w-full",
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
var stepIndicatorVariants = (0, import_class_variance_authority3.cva)(
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
var stepTitleVariants = (0, import_class_variance_authority3.cva)(["text-[var(--ui-text-sm)] font-medium"], {
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
});
var stepDescriptionVariants = (0, import_class_variance_authority3.cva)(["text-[var(--ui-text-xs)]", "text-[var(--ui-text-muted)]"]);
var contentVariants = (0, import_class_variance_authority3.cva)(
  ["overflow-auto", "p-[var(--ui-panel-padding)]", "transition-all duration-150 ease-out"],
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
var footerVariants = (0, import_class_variance_authority3.cva)([
  "flex flex-wrap items-center justify-between gap-2",
  "px-[var(--ui-panel-padding)] py-[var(--ui-space-4)]",
  "border-t border-[var(--ui-border)]",
  "bg-[var(--ui-bg-surface)]"
]);
var Wizard = (0, import_react12.forwardRef)(
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
    const [animatingStep, setAnimatingStep] = (0, import_react12.useState)(null);
    (0, import_react12.useEffect)(() => {
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
    const handleNext = (0, import_react12.useCallback)(() => {
      if (currentStep < steps.length - 1 && canProceed && !loading) {
        setAnimatingStep("next");
        setTimeout(() => {
          onStepChange?.(currentStep + 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, steps.length, canProceed, loading, onStepChange]);
    const handlePrevious = (0, import_react12.useCallback)(() => {
      if (currentStep > 0 && !loading) {
        setAnimatingStep("prev");
        setTimeout(() => {
          onStepChange?.(currentStep - 1);
          setAnimatingStep(null);
        }, 150);
      }
    }, [currentStep, loading, onStepChange]);
    const handleStepClick = (0, import_react12.useCallback)(
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
    const handleComplete = (0, import_react12.useCallback)(() => {
      if (canProceed && !loading) {
        onComplete?.();
      }
    }, [canProceed, loading, onComplete]);
    const handleCancel = (0, import_react12.useCallback)(() => {
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
    const dialog = /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { className: cn("ui-wizard-overlay", overlayVariants()), children: [
      /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
        "button",
        {
          type: "button",
          className: "ui-wizard-dismiss",
          "aria-label": "Close wizard",
          tabIndex: -1,
          onClick: onClose
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)(
        "div",
        {
          ref,
          className: cn(
            "ui-wizard",
            `ui-wizard--${size ?? "md"}`,
            wizardVariants({ size, className })
          ),
          role: "dialog",
          "aria-modal": "true",
          "aria-labelledby": "wizard-title",
          ...props,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("header", { className: cn("ui-wizard-header", headerVariants()), children: [
              /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("h2", { id: "wizard-title", className: cn(titleVariants()), children: title }),
                subtitle && /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("p", { className: cn("ui-wizard-subtitle", subtitleVariants()), children: subtitle })
              ] }),
              onClose && /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                "button",
                {
                  type: "button",
                  className: cn("ui-wizard-close", closeButtonVariants()),
                  onClick: onClose,
                  "aria-label": "Close wizard",
                  disabled: loading,
                  children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                    "svg",
                    {
                      "aria-hidden": "true",
                      className: "w-5 h-5",
                      viewBox: "0 0 24 24",
                      fill: "none",
                      stroke: "currentColor",
                      strokeWidth: "2",
                      strokeLinecap: "round",
                      strokeLinejoin: "round",
                      children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("path", { d: "M18 6L6 18M6 6l12 12" })
                    }
                  )
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("nav", { className: cn("ui-wizard-steps", stepsNavVariants()), "aria-label": "Wizard steps", children: [
              /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { className: cn("ui-wizard-progress", progressTrackVariants()), children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                "div",
                {
                  className: cn(progressFillVariants()),
                  style: { width: `${progressPercent}%` }
                }
              ) }),
              /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("ol", { className: cn(stepListVariants()), children: steps.map((step, index) => {
                const state = getStepState(index);
                const isClickable = allowStepClick && (state === "completed" || canProceed && index === currentStep + 1);
                return /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                  "li",
                  {
                    className: cn(stepItemVariants({ state, clickable: isClickable })),
                    children: /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)(
                      "button",
                      {
                        type: "button",
                        className: cn(stepButtonVariants({ state, clickable: isClickable })),
                        onClick: () => handleStepClick(index),
                        disabled: !isClickable || loading,
                        "aria-current": state === "current" ? "step" : void 0,
                        children: [
                          /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: cn(stepIndicatorVariants({ state })), children: state === "completed" ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                            "svg",
                            {
                              "aria-hidden": "true",
                              className: "w-4 h-4",
                              viewBox: "0 0 24 24",
                              fill: "none",
                              stroke: "currentColor",
                              strokeWidth: "3",
                              strokeLinecap: "round",
                              strokeLinejoin: "round",
                              children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("polyline", { points: "20 6 9 17 4 12" })
                            }
                          ) : step.icon ? step.icon : showStepNumbers ? index + 1 : /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: "w-2 h-2 rounded-full bg-current" }) }),
                          /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("span", { className: "ui-wizard-step-copy flex flex-col items-start", children: [
                            /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("span", { className: cn("ui-wizard-step-title", stepTitleVariants({ state })), children: step.title }),
                            step.description && /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                              "span",
                              {
                                className: cn("ui-wizard-step-description", stepDescriptionVariants()),
                                children: step.description
                              }
                            )
                          ] })
                        ]
                      }
                    )
                  },
                  step.id
                );
              }) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
              "div",
              {
                className: cn(
                  "ui-wizard-content",
                  contentVariants({ animating: animatingStep || "none" })
                ),
                children
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("footer", { className: cn("ui-wizard-footer", footerVariants()), children: [
              /* @__PURE__ */ (0, import_jsx_runtime12.jsx)("div", { children: /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(Button, { variant: "ghost", onClick: handleCancel, disabled: loading, children: cancelLabel }) }),
              /* @__PURE__ */ (0, import_jsx_runtime12.jsxs)("div", { className: "flex items-center gap-[var(--ui-space-3)]", children: [
                !isFirstStep && /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(Button, { variant: "secondary", onClick: handlePrevious, disabled: loading, children: previousLabel }),
                isLastStep ? /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(
                  Button,
                  {
                    variant: "primary",
                    onClick: handleComplete,
                    disabled: !canProceed || loading,
                    loading,
                    children: completeLabel
                  }
                ) : /* @__PURE__ */ (0, import_jsx_runtime12.jsx)(Button, { variant: "primary", onClick: handleNext, disabled: !canProceed || loading, children: nextLabel })
              ] })
            ] })
          ]
        }
      )
    ] });
    if (typeof document === "undefined") {
      return dialog;
    }
    return (0, import_react_dom.createPortal)(dialog, document.body);
  }
);
Wizard.displayName = "Wizard";

// src/components/Card/Card.tsx
var import_react13 = __toESM(require("react"));
var import_jsx_runtime13 = require("react/jsx-runtime");
var Card = import_react13.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { ref, className: cn("ui-card", className), ...props })
);
Card.displayName = "Card";
var CardHeader = import_react13.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { ref, className: cn("ui-card-header", className), ...props })
);
CardHeader.displayName = "CardHeader";
var CardTitle = import_react13.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("h3", { ref, className: cn("ui-card-title", className), ...props })
);
CardTitle.displayName = "CardTitle";
var CardDescription = import_react13.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("p", { ref, className: cn("ui-card-description", className), ...props })
);
CardDescription.displayName = "CardDescription";
var CardContent = import_react13.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { ref, className: cn("ui-card-content", className), ...props })
);
CardContent.displayName = "CardContent";
var CardFooter = import_react13.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime13.jsx)("div", { ref, className: cn("ui-card-footer", className), ...props })
);
CardFooter.displayName = "CardFooter";

// src/components/Separator/Separator.tsx
var import_react14 = __toESM(require("react"));
var SeparatorPrimitive = __toESM(require("@radix-ui/react-separator"));
var import_jsx_runtime14 = require("react/jsx-runtime");
var Separator2 = import_react14.default.forwardRef(({ className, orientation = "horizontal", decorative = true, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime14.jsx)(
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
var import_react15 = __toESM(require("react"));
var import_jsx_runtime15 = require("react/jsx-runtime");
var SectionHeader = import_react15.default.forwardRef(
  ({ title, size = "md", className, ...props }, ref) => {
    const textSize = size === "sm" ? "ui-section-title-sm" : void 0;
    return /* @__PURE__ */ (0, import_jsx_runtime15.jsxs)("div", { ref, className: cn("ui-section-header", className), ...props, children: [
      /* @__PURE__ */ (0, import_jsx_runtime15.jsx)("h3", { className: cn("ui-section-title", textSize), children: title }),
      /* @__PURE__ */ (0, import_jsx_runtime15.jsx)(Separator2, { className: "flex-1" })
    ] });
  }
);
SectionHeader.displayName = "SectionHeader";

// src/components/Table/Table.tsx
var import_react16 = __toESM(require("react"));
var import_jsx_runtime16 = require("react/jsx-runtime");
var Table = import_react16.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("div", { className: "w-full overflow-auto", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("table", { ref, className: cn("w-full caption-bottom text-sm", className), ...props }) })
);
Table.displayName = "Table";
var TableHeader = import_react16.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("thead", { ref, className: cn("[&_tr]:border-b", className), ...props }));
TableHeader.displayName = "TableHeader";
var TableBody = import_react16.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("tbody", { ref, className: cn("[&_tr:last-child]:border-0", className), ...props }));
TableBody.displayName = "TableBody";
var TableFooter = import_react16.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)(
  "tfoot",
  {
    ref,
    className: cn("border-t bg-muted/50 font-medium [&>tr]:last:border-b-0", className),
    ...props
  }
));
TableFooter.displayName = "TableFooter";
var TableRow = import_react16.default.forwardRef(
  ({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)(
    "tr",
    {
      ref,
      className: cn(
        "border-b transition-colors hover:bg-muted/50 data-[state=selected]:bg-muted",
        className
      ),
      ...props
    }
  )
);
TableRow.displayName = "TableRow";
var TableHead = import_react16.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)(
  "th",
  {
    ref,
    className: cn(
      "ui-table-head text-left align-middle font-medium text-muted-foreground [&:has([role=checkbox])]:pr-0",
      className
    ),
    ...props
  }
));
TableHead.displayName = "TableHead";
var TableCell = import_react16.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)(
  "td",
  {
    ref,
    className: cn("ui-table-cell align-middle [&:has([role=checkbox])]:pr-0", className),
    ...props
  }
));
TableCell.displayName = "TableCell";
var TableCaption = import_react16.default.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("caption", { ref, className: cn("mt-4 text-sm text-muted-foreground", className), ...props }));
TableCaption.displayName = "TableCaption";

// src/components/Badge/Badge.tsx
var import_class_variance_authority4 = require("class-variance-authority");
var import_jsx_runtime17 = require("react/jsx-runtime");
var badgeVariants = (0, import_class_variance_authority4.cva)(
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
  return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("div", { className: cn(badgeVariants({ variant }), className), ...props });
}

// src/components/Tabs/Tabs.tsx
var React17 = __toESM(require("react"));
var TabsPrimitive = __toESM(require("@radix-ui/react-tabs"));
var import_jsx_runtime18 = require("react/jsx-runtime");
var Tabs = TabsPrimitive.Root;
var TabsList = React17.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
  TabsPrimitive.List,
  {
    ref,
    className: cn("ui-tabs-list", className),
    ...props
  }
));
TabsList.displayName = TabsPrimitive.List.displayName;
var TabsTrigger = React17.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
  TabsPrimitive.Trigger,
  {
    ref,
    className: cn("ui-tabs-trigger", className),
    ...props
  }
));
TabsTrigger.displayName = TabsPrimitive.Trigger.displayName;
var TabsContent = React17.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
  TabsPrimitive.Content,
  {
    ref,
    className: cn("ui-tabs-content", className),
    ...props
  }
));
TabsContent.displayName = TabsPrimitive.Content.displayName;

// src/components/EmptyState/EmptyState.tsx
var import_jsx_runtime19 = require("react/jsx-runtime");
function EmptyState({ icon, title, description, action, className = "" }) {
  return /* @__PURE__ */ (0, import_jsx_runtime19.jsxs)("div", { className: `ui-empty ${className}`, children: [
    icon && /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("div", { className: "ui-empty-icon", children: icon }),
    /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("h3", { className: "ui-empty-title", children: title }),
    description && /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("p", { className: "ui-empty-description", children: description }),
    action && /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("div", { className: "ui-empty-action", children: action })
  ] });
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
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
  Separator,
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
});
