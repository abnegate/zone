import { buttonVariants } from '@zone/ui';
import { format } from 'date-fns';
import type { RefObject } from 'react';

interface ClaudeLinkProps {
  url: string;
  full: boolean;
  expiresAt: string;
  entry?: RefObject<HTMLAnchorElement | null>;
}

/** The step that opens claude.com to approve a sign-in, and when its link expires. */
export function ClaudeLink({ url, full, expiresAt, entry }: ClaudeLinkProps) {
  const expiry = format(new Date(expiresAt), 'p');
  return (
    <>
      <div className="agent-sign-in-step">
        <span>Open claude.com, sign in, and approve access.</span>
        <a
          ref={entry}
          className={buttonVariants({ variant: 'secondary', size: 'sm' })}
          href={url}
          target="_blank"
          rel="noopener noreferrer"
        >
          Open claude.com
        </a>
      </div>
      <p className="form-hint">
        {full
          ? `This link asks for full access to your Claude account. It expires at ${expiry}.`
          : `The link expires at ${expiry}.`}
      </p>
    </>
  );
}
