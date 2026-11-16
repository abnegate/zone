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

  it('names each attached source kind the way the source registry does', () => {
    render(
      <ChatSources
        attached={[
          { id: 'source-1', name: 'notes', source_type: 'filesystem' },
          { id: 'source-2', name: 'feed', source_type: 'carrier_pigeon' },
        ]}
        available={[]}
        loading={false}
        error={null}
        onChange={async () => {}}
      />
    );

    const kinds = screen
      .getAllByTestId('chat-source-chip')
      .map((chip) => chip.firstChild?.textContent);
    expect(kinds).toEqual(['Filesystem', 'carrier_pigeon']);
  });
});
