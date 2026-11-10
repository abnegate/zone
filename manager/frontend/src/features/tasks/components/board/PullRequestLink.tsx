import { CardSensor } from './CardSensor';

export interface PullRequestLinkProps {
  url: string;
}

const WEB_PROTOCOLS: ReadonlySet<string> = new Set(['http:', 'https:']);

function protocol(url: string): string | null {
  try {
    return new URL(url).protocol;
  } catch {
    return null;
  }
}

export function PullRequestLink({ url }: PullRequestLinkProps) {
  if (!WEB_PROTOCOLS.has(protocol(url) ?? '')) return null;
  return (
    <a
      className="task-card-pr"
      href={url}
      target="_blank"
      rel="noopener noreferrer"
      onPointerDown={CardSensor.shield}
      onClick={CardSensor.shield}
      onKeyDown={CardSensor.shield}
    >
      View PR
    </a>
  );
}
