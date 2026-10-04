import type { FormField, FormRow } from '../config';

export function FormFieldRenderer({
  field,
  value,
  onChange,
}: {
  field: FormField;
  value: unknown;
  onChange: (id: string, value: unknown) => void;
}) {
  if (field.type === 'toggle') {
    return (
      <div className="form-group">
        <label className="toggle-label">
          <span className="toggle-wrapper">
            <input
              type="checkbox"
              checked={value as boolean}
              onChange={(e) => onChange(field.id, e.target.checked)}
            />
            <span className="toggle-slider" />
          </span>
          <span className="toggle-text">
            <span className="toggle-title">{field.toggleTitle || field.label}</span>
            {field.toggleDescription && (
              <span className="toggle-desc">{field.toggleDescription}</span>
            )}
          </span>
        </label>
      </div>
    );
  }

  if (field.type === 'textarea') {
    return (
      <div className="form-group">
        <label htmlFor={field.id}>
          {field.label}
          {!field.required && <span className="label-optional">optional</span>}
        </label>
        <textarea
          id={field.id}
          value={value as string}
          onChange={(e) => onChange(field.id, e.target.value)}
          placeholder={field.placeholder}
          required={field.required}
          rows={6}
        />
        {field.hint && <span className="form-hint">{field.hint}</span>}
      </div>
    );
  }

  return (
    <div className="form-group">
      <label htmlFor={field.id}>
        {field.label}
        {!field.required && <span className="label-optional">optional</span>}
      </label>
      <input
        type={field.type}
        id={field.id}
        value={value as string | number}
        onChange={(e) =>
          onChange(
            field.id,
            field.type === 'number' ? Number.parseInt(e.target.value, 10) || 0 : e.target.value
          )
        }
        placeholder={field.placeholder}
        required={field.required}
        className={field.monospace ? 'input-mono' : undefined}
      />
      {field.hint && <span className="form-hint">{field.hint}</span>}
    </div>
  );
}

export function FormFieldsRenderer({
  fields,
  state,
  onChange,
}: {
  fields: (FormField | FormRow)[];
  state: Record<string, unknown>;
  onChange: (id: string, value: unknown) => void;
}) {
  return (
    <>
      {fields.map((item) => {
        if ('fields' in item) {
          const rowKey = item.fields.map((f) => f.id).join('-');
          return (
            <div key={rowKey} className="form-row">
              {item.fields.map((field) => (
                <FormFieldRenderer
                  key={field.id}
                  field={field}
                  value={state[field.id]}
                  onChange={onChange}
                />
              ))}
            </div>
          );
        }
        return (
          <FormFieldRenderer
            key={item.id}
            field={item}
            value={state[item.id]}
            onChange={onChange}
          />
        );
      })}
    </>
  );
}

export function withCredentialField(
  fields: (FormField | FormRow)[],
  credentialField: FormField | undefined
): (FormField | FormRow)[] {
  if (!credentialField) return fields;
  const last = fields[fields.length - 1];
  if (last && 'fields' in last && last.fields.length === 1) {
    return [...fields.slice(0, -1), { fields: [...last.fields, credentialField] }];
  }
  return [...fields, credentialField];
}
