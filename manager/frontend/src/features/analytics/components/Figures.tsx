export interface Figure {
  label: string;
  value: string;
}

export interface FiguresProps {
  label: string;
  figures: readonly Figure[];
}

export function Figures({ label, figures }: FiguresProps) {
  return (
    <dl className="analytics-figures" aria-label={label}>
      {figures.map((figure) => (
        <div key={figure.label} className="analytics-figure">
          <dt className="analytics-figure-label">{figure.label}</dt>
          <dd className="analytics-figure-value">{figure.value}</dd>
        </div>
      ))}
    </dl>
  );
}
