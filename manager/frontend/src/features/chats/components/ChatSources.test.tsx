import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { SOURCES_LOCATION } from '../../settings/navigation/sections';
import { ChatSources } from './ChatSources';

describe('ChatSources', () => {
  it('points an empty workspace at the sources settings section', () => {
    render(
      <ChatSources
        attached={[]}
        available={[]}
        loading={false}
        error={null}
        onChange={async () => {}}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Sources: whole workspace' }));

    expect(
      screen.getByText(
        `No sources in this workspace yet. Connect a repository, folder or page in ${SOURCES_LOCATION} first.`
      )
    ).toBeInTheDocument();
  });
});
