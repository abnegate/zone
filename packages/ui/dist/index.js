"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
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
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/index.ts
var index_exports = {};
__export(index_exports, {
  Button: () => Button,
  Checkbox: () => Checkbox,
  InfoBox: () => InfoBox,
  Input: () => Input,
  Modal: () => Modal,
  ProgressBar: () => ProgressBar,
  Select: () => Select
});
module.exports = __toCommonJS(index_exports);

// src/components/Button/Button.tsx
var import_react = require("react");
var import_jsx_runtime = require("react/jsx-runtime");
var Button = (0, import_react.forwardRef)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
      "button",
      {
        ref,
        className: classes,
        disabled: disabled || loading,
        type,
        ...props,
        children: [
          loading && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "ui-btn__spinner", "aria-hidden": "true" }),
          children
        ]
      }
    );
  }
);
Button.displayName = "Button";

// src/components/Input/Input.tsx
var import_react2 = require("react");
var import_jsx_runtime2 = require("react/jsx-runtime");
var Input = (0, import_react2.forwardRef)(
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
    const inputElement = /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
      "input",
      {
        ref,
        id: inputId,
        className: inputClasses,
        type,
        ...props
      }
    );
    return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "ui-form-field", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("label", { className: "ui-form-field__label", htmlFor: inputId, children: label }),
      onGenerate ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "ui-input-wrapper", children: [
        inputElement,
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Button, { variant: "generate", type: "button", onClick: onGenerate, children: "Generate" })
      ] }) : inputElement,
      error && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "ui-form-field__error", children: error }),
      helpText && !error && /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("p", { className: "ui-form-field__help", children: helpText })
    ] });
  }
);
Input.displayName = "Input";

// src/components/Select/Select.tsx
var import_react3 = require("react");
var import_jsx_runtime3 = require("react/jsx-runtime");
var Select = (0, import_react3.forwardRef)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("div", { className: "ui-form-field", children: [
      /* @__PURE__ */ (0, import_jsx_runtime3.jsx)("label", { className: "ui-form-field__label", htmlFor: selectId, children: label }),
      /* @__PURE__ */ (0, import_jsx_runtime3.jsx)(
        "select",
        {
          ref,
          id: selectId,
          className: selectClasses,
          ...props,
          children: options.map((option) => /* @__PURE__ */ (0, import_jsx_runtime3.jsx)("option", { value: option.value, children: option.label }, option.value))
        }
      ),
      error && /* @__PURE__ */ (0, import_jsx_runtime3.jsx)("p", { className: "ui-form-field__error", children: error }),
      helpText && !error && /* @__PURE__ */ (0, import_jsx_runtime3.jsx)("p", { className: "ui-form-field__help", children: helpText })
    ] });
  }
);
Select.displayName = "Select";

// src/components/Checkbox/Checkbox.tsx
var import_react4 = require("react");
var import_jsx_runtime4 = require("react/jsx-runtime");
var Checkbox = (0, import_react4.forwardRef)(
  ({ label, helpText, id, className = "", ...props }, ref) => {
    const checkboxId = id || label.toLowerCase().replace(/\s+/g, "-");
    return /* @__PURE__ */ (0, import_jsx_runtime4.jsxs)("div", { className: "ui-form-field ui-form-field--checkbox", children: [
      /* @__PURE__ */ (0, import_jsx_runtime4.jsxs)("label", { className: `ui-checkbox ${className}`.trim(), htmlFor: checkboxId, children: [
        /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
          "input",
          {
            ref,
            type: "checkbox",
            id: checkboxId,
            className: "ui-checkbox__input",
            ...props
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("span", { className: "ui-checkbox__label", children: label })
      ] }),
      helpText && /* @__PURE__ */ (0, import_jsx_runtime4.jsx)("p", { className: "ui-form-field__help", children: helpText })
    ] });
  }
);
Checkbox.displayName = "Checkbox";

// src/components/Modal/Modal.tsx
var import_react5 = require("react");
var import_jsx_runtime5 = require("react/jsx-runtime");
var Modal = (0, import_react5.forwardRef)(
  ({ isOpen, onClose, title, children, className = "", ...props }, ref) => {
    (0, import_react5.useEffect)(() => {
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
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "ui-modal-overlay", onClick: onClose, children: /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(
      "div",
      {
        ref,
        className: `ui-modal ${className}`.trim(),
        onClick: (e) => e.stopPropagation(),
        ...props,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("h3", { className: "ui-modal__title", children: title }),
          children
        ]
      }
    ) });
  }
);
Modal.displayName = "Modal";

// src/components/InfoBox/InfoBox.tsx
var import_react6 = require("react");
var import_jsx_runtime6 = require("react/jsx-runtime");
var InfoBox = (0, import_react6.forwardRef)(
  ({ variant = "info", children, className = "", ...props }, ref) => {
    const classes = [
      "ui-info-box",
      `ui-info-box--${variant}`,
      className
    ].filter(Boolean).join(" ");
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { ref, className: classes, ...props, children });
  }
);
InfoBox.displayName = "InfoBox";

// src/components/ProgressBar/ProgressBar.tsx
var import_react7 = require("react");
var import_jsx_runtime7 = require("react/jsx-runtime");
var ProgressBar = (0, import_react7.forwardRef)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime7.jsxs)("div", { ref, className: classes, ...props, children: [
      (label || showPercentage) && /* @__PURE__ */ (0, import_jsx_runtime7.jsxs)("div", { className: "ui-progress__header", children: [
        /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("span", { children: label }),
        showPercentage && /* @__PURE__ */ (0, import_jsx_runtime7.jsxs)("span", { children: [
          percentage,
          "%"
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime7.jsx)("div", { className: "ui-progress__track", children: /* @__PURE__ */ (0, import_jsx_runtime7.jsx)(
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
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  Button,
  Checkbox,
  InfoBox,
  Input,
  Modal,
  ProgressBar,
  Select
});
