import { render, screen } from '@testing-library/react';
import { StatusLog } from './StatusLog';

describe('StatusLog', () => {
  it('renders lines', () => {
    const lines = [
      { message: 'Starting installation...' },
      { message: 'Downloading files...' },
      { message: 'Installation complete!' },
    ];

    render(<StatusLog lines={lines} />);

    expect(screen.getByText('Starting installation...')).toBeInTheDocument();
    expect(screen.getByText('Downloading files...')).toBeInTheDocument();
    expect(screen.getByText('Installation complete!')).toBeInTheDocument();
  });

  it('applies type class to lines', () => {
    const lines = [
      { message: 'Normal message' },
      { message: 'Success!', type: 'success' as const },
      { message: 'Error occurred', type: 'error' as const },
      { message: 'Retrying...', type: 'retry' as const },
    ];

    render(<StatusLog lines={lines} />);

    const lineElements = document.querySelectorAll('.status-line');

    expect(lineElements[0]).not.toHaveClass('success');
    expect(lineElements[0]).not.toHaveClass('error');
    expect(lineElements[0]).not.toHaveClass('retry');

    expect(lineElements[1]).toHaveClass('success');
    expect(lineElements[2]).toHaveClass('error');
    expect(lineElements[3]).toHaveClass('retry');
  });

  it('handles empty lines array', () => {
    render(<StatusLog lines={[]} />);

    const container = document.querySelector('.status-log');
    expect(container).toBeInTheDocument();
    expect(container?.children.length).toBe(0);
  });

  it('auto-scrolls to bottom when lines change', () => {
    const lines = [{ message: 'Line 1' }];

    const { rerender } = render(<StatusLog lines={lines} />);

    const container = document.querySelector('.status-log') as HTMLDivElement;
    // Mock scrollHeight and scrollTop
    Object.defineProperty(container, 'scrollHeight', { value: 200 });
    Object.defineProperty(container, 'scrollTop', { value: 0, writable: true });

    const newLines = [...lines, { message: 'Line 2' }, { message: 'Line 3' }];
    rerender(<StatusLog lines={newLines} />);

    expect(container.scrollTop).toBe(200);
  });

  it('renders lines in order', () => {
    const lines = [{ message: 'First' }, { message: 'Second' }, { message: 'Third' }];

    render(<StatusLog lines={lines} />);

    const lineElements = document.querySelectorAll('.status-line');
    expect(lineElements[0]).toHaveTextContent('First');
    expect(lineElements[1]).toHaveTextContent('Second');
    expect(lineElements[2]).toHaveTextContent('Third');
  });

  it('handles lines with normal type', () => {
    const lines = [{ message: 'Normal line', type: 'normal' as const }];

    render(<StatusLog lines={lines} />);

    const lineElement = document.querySelector('.status-line');
    expect(lineElement).toHaveClass('normal');
  });
});
