import { useId, useMemo } from 'react';
import { useGitHubBranches } from '../hooks';
import type { GitHubRepository } from '../types';

const FALLBACK_BRANCH = 'main';

type BranchSelectProps = {
  organizationId: string | null;
  repository: GitHubRepository | null;
  value: string;
  onChange: (branch: string) => void;
};

export function BranchSelect({ organizationId, repository, value, onChange }: BranchSelectProps) {
  const inputId = useId();
  const datalistId = useId();
  const errorId = useId();
  const { branches, loading, failed } = useGitHubBranches(organizationId, repository);

  const names = useMemo(() => {
    const defaultBranch = repository?.default_branch;
    const others = branches.map((branch) => branch.name).filter((name) => name !== defaultBranch);
    return defaultBranch ? [defaultBranch, ...others] : others;
  }, [branches, repository]);

  return (
    <div className="form-group">
      <label htmlFor={inputId}>Branch</label>
      <input
        id={inputId}
        type="text"
        list={datalistId}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={repository?.default_branch ?? FALLBACK_BRANCH}
        disabled={repository === null}
        autoComplete="off"
        spellCheck={false}
        aria-busy={loading}
        aria-describedby={failed ? errorId : undefined}
      />
      <datalist id={datalistId}>
        {names.map((name) => (
          <option key={name} value={name} />
        ))}
      </datalist>
      {failed && (
        <p id={errorId} role="alert" className="field-error">
          Couldn't load branches. Type the branch name instead.
        </p>
      )}
    </div>
  );
}
