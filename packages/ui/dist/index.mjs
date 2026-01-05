// src/components/Button/Button.tsx
import { forwardRef } from "react";
import { jsx, jsxs } from "react/jsx-runtime";
var Button = forwardRef(
  ({
    variant = "primary",
    size = "md",
    loading = false,
    children,
    className = "",
    disabled,
    type = "button",
    ...props
  }, ref) => {
    const classes = [
      "ui-btn",
      `ui-btn--${variant}`,
      `ui-btn--${size}`,
      className
    ].filter(Boolean).join(" ");
    return /* @__PURE__ */ jsxs(
      "button",
      {
        ref,
        className: classes,
        disabled: disabled || loading,
        type,
        ...props,
        children: [
          loading && /* @__PURE__ */ jsx("span", { className: "ui-btn__spinner", "aria-hidden": "true" }),
          children
        ]
      }
    );
  }
);
Button.displayName = "Button";

// src/components/Input/Input.tsx
import { forwardRef as forwardRef2 } from "react";
import { jsx as jsx2, jsxs as jsxs2 } from "react/jsx-runtime";
var Input = forwardRef2(
  ({
    label,
    helpText,
    error,
    onGenerate,
    id,
    className = "",
    type = "text",
    ...props
  }, ref) => {
    const inputId = id || label.toLowerCase().replace(/\s+/g, "-");
    const inputClasses = [
      "ui-input",
      error ? "ui-input--error" : "",
      className
    ].filter(Boolean).join(" ");
    const inputElement = /* @__PURE__ */ jsx2(
      "input",
      {
        ref,
        id: inputId,
        className: inputClasses,
        type,
        ...props
      }
    );
    return /* @__PURE__ */ jsxs2("div", { className: "ui-form-field", children: [
      /* @__PURE__ */ jsx2("label", { className: "ui-form-field__label", htmlFor: inputId, children: label }),
      onGenerate ? /* @__PURE__ */ jsxs2("div", { className: "ui-input-wrapper", children: [
        inputElement,
        /* @__PURE__ */ jsx2(Button, { variant: "generate", type: "button", onClick: onGenerate, children: "Generate" })
      ] }) : inputElement,
      error && /* @__PURE__ */ jsx2("p", { className: "ui-form-field__error", children: error }),
      helpText && !error && /* @__PURE__ */ jsx2("p", { className: "ui-form-field__help", children: helpText })
    ] });
  }
);
Input.displayName = "Input";

// src/components/Select/Select.tsx
import { forwardRef as forwardRef3 } from "react";
import { jsx as jsx3, jsxs as jsxs3 } from "react/jsx-runtime";
var Select = forwardRef3(
  ({
    label,
    options,
    helpText,
    error,
    id,
    className = "",
    ...props
  }, ref) => {
    const selectId = id || label.toLowerCase().replace(/\s+/g, "-");
    const selectClasses = [
      "ui-select",
      error ? "ui-select--error" : "",
      className
    ].filter(Boolean).join(" ");
    return /* @__PURE__ */ jsxs3("div", { className: "ui-form-field", children: [
      /* @__PURE__ */ jsx3("label", { className: "ui-form-field__label", htmlFor: selectId, children: label }),
      /* @__PURE__ */ jsx3(
        "select",
        {
          ref,
          id: selectId,
          className: selectClasses,
          ...props,
          children: options.map((option) => /* @__PURE__ */ jsx3("option", { value: option.value, children: option.label }, option.value))
        }
      ),
      error && /* @__PURE__ */ jsx3("p", { className: "ui-form-field__error", children: error }),
      helpText && !error && /* @__PURE__ */ jsx3("p", { className: "ui-form-field__help", children: helpText })
    ] });
  }
);
Select.displayName = "Select";

// src/components/Checkbox/Checkbox.tsx
import { forwardRef as forwardRef4 } from "react";
import { jsx as jsx4, jsxs as jsxs4 } from "react/jsx-runtime";
var Checkbox = forwardRef4(
  ({ label, id, className = "", ...props }, ref) => {
    const checkboxId = id || label.toLowerCase().replace(/\s+/g, "-");
    return /* @__PURE__ */ jsxs4("label", { className: `ui-checkbox ${className}`.trim(), htmlFor: checkboxId, children: [
      /* @__PURE__ */ jsx4(
        "input",
        {
          ref,
          type: "checkbox",
          id: checkboxId,
          className: "ui-checkbox__input",
          ...props
        }
      ),
      /* @__PURE__ */ jsx4("span", { className: "ui-checkbox__label", children: label })
    ] });
  }
);
Checkbox.displayName = "Checkbox";

// src/components/Modal/Modal.tsx
import { forwardRef as forwardRef5, useEffect } from "react";
import { jsx as jsx5, jsxs as jsxs5 } from "react/jsx-runtime";
var Modal = forwardRef5(
  ({ isOpen, onClose, title, children, className = "", ...props }, ref) => {
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
    return /* @__PURE__ */ jsx5("div", { className: "ui-modal-overlay", onClick: onClose, children: /* @__PURE__ */ jsxs5(
      "div",
      {
        ref,
        className: `ui-modal ${className}`.trim(),
        onClick: (e) => e.stopPropagation(),
        ...props,
        children: [
          /* @__PURE__ */ jsx5("h3", { className: "ui-modal__title", children: title }),
          children
        ]
      }
    ) });
  }
);
Modal.displayName = "Modal";

// src/components/InfoBox/InfoBox.tsx
import { forwardRef as forwardRef6 } from "react";
import { jsx as jsx6 } from "react/jsx-runtime";
var InfoBox = forwardRef6(
  ({ variant = "info", children, className = "", ...props }, ref) => {
    const classes = [
      "ui-info-box",
      `ui-info-box--${variant}`,
      className
    ].filter(Boolean).join(" ");
    return /* @__PURE__ */ jsx6("div", { ref, className: classes, ...props, children });
  }
);
InfoBox.displayName = "InfoBox";

// src/components/ProgressBar/ProgressBar.tsx
import { forwardRef as forwardRef7 } from "react";
import { jsx as jsx7, jsxs as jsxs6 } from "react/jsx-runtime";
var ProgressBar = forwardRef7(
  ({
    value,
    max = 100,
    label,
    showPercentage = true,
    thin = false,
    className = "",
    ...props
  }, ref) => {
    const percentage = Math.round(value / max * 100);
    const classes = [
      "ui-progress",
      thin ? "ui-progress--thin" : "",
      className
    ].filter(Boolean).join(" ");
    return /* @__PURE__ */ jsxs6("div", { ref, className: classes, ...props, children: [
      (label || showPercentage) && /* @__PURE__ */ jsxs6("div", { className: "ui-progress__header", children: [
        /* @__PURE__ */ jsx7("span", { children: label }),
        showPercentage && /* @__PURE__ */ jsxs6("span", { children: [
          percentage,
          "%"
        ] })
      ] }),
      /* @__PURE__ */ jsx7("div", { className: "ui-progress__track", children: /* @__PURE__ */ jsx7(
        "div",
        {
          className: "ui-progress__fill",
          style: { width: `${percentage}%` }
        }
      ) })
    ] });
  }
);
ProgressBar.displayName = "ProgressBar";
export {
  Button,
  Checkbox,
  InfoBox,
  Input,
  Modal,
  ProgressBar,
  Select
};
