import { CardSensor } from './CardSensor';

export interface PullRequestLinkProps {
  url: string;
}

export function PullRequestLink({ url }: PullRequestLinkProps) {
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
