import { Icon } from '../../github';
import { EXTERNAL_PATH } from './icons';

export interface GitHubLinkProps {
  href: string;
  label: string;
}

export function GitHubLink({ href, label }: GitHubLinkProps) {
  return (
    <a
      className="pull-request-link"
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={label}
      title="Open on GitHub"
    >
      <Icon path={EXTERNAL_PATH} />
    </a>
  );
}
