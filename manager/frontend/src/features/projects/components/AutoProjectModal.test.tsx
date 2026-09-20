import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { AutoProjectModal } from './AutoProjectModal';

describe('AutoProjectModal', () => {
  it('renders nothing while closed', () => {
    render(
      <AutoProjectModal
        isOpen={false}
        onClose={() => {}}
        start={() => Promise.resolve({ chat_id: 'c' })}
        onStarted={() => {}}
      />
    );
    expect(screen.queryByTestId('auto-project-modal')).not.toBeInTheDocument();
  });

  it('refuses an empty brief without calling the server', async () => {
    const start = mock(() => Promise.resolve({ chat_id: 'c' }));
    render(<AutoProjectModal isOpen onClose={() => {}} start={start} onStarted={() => {}} />);

    const submit = screen.getByRole('button', { name: 'Start the interview' });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByTestId('auto-project-brief'), { target: { value: '   ' } });
    expect(submit).toBeDisabled();
    expect(start).not.toHaveBeenCalled();
  });

  it('trims the brief, starts the interview and reports the chat', async () => {
    const start = mock(() => Promise.resolve({ chat_id: 'chat-42' }));
    const onStarted = mock(() => {});
    render(<AutoProjectModal isOpen onClose={() => {}} start={start} onStarted={onStarted} />);

    fireEvent.change(screen.getByTestId('auto-project-brief'), {
      target: { value: '  A habit tracker for the web  ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Start the interview' }));

    await waitFor(() => {
      expect(onStarted).toHaveBeenCalledWith('chat-42');
    });
    expect(start).toHaveBeenCalledWith({ brief: 'A habit tracker for the web' });
  });

  it('shows the failure and keeps the brief', async () => {
    const start = mock(() => Promise.reject(new Error('No model is installed')));
    const onStarted = mock(() => {});
    render(<AutoProjectModal isOpen onClose={() => {}} start={start} onStarted={onStarted} />);

    fireEvent.change(screen.getByTestId('auto-project-brief'), {
      target: { value: 'A habit tracker' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Start the interview' }));

    await waitFor(() => {
      expect(screen.getByText('No model is installed')).toBeInTheDocument();
    });
    expect(onStarted).not.toHaveBeenCalled();
    expect(screen.getByTestId('auto-project-brief')).toHaveValue('A habit tracker');
  });

  it('closes from the cancel button', () => {
    const onClose = mock(() => {});
    render(
      <AutoProjectModal
        isOpen
        onClose={onClose}
        start={() => Promise.resolve({ chat_id: 'c' })}
        onStarted={() => {}}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalled();
  });
});
