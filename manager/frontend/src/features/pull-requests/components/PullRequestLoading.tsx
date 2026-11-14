export interface PullRequestLoadingProps {
  label: string;
}

export function PullRequestLoading({ label }: PullRequestLoadingProps) {
  return (
    <p className="pull-request-muted" role="status">
      <span className="spinner" />
      {label}
    </p>
  );
}
