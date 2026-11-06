const HEX_PATTERN = '^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$';

export default function ColorField({
  id,
  label,
  value,
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="form-group">
      <label htmlFor={id}>{label}</label>
      <div className="color-input-wrapper">
        <input
          type="color"
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
        <input
          type="text"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          pattern={HEX_PATTERN}
          className="color-text-input"
          aria-label={`${label} hex`}
        />
      </div>
    </div>
  );
}
