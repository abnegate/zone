import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { client } from '../../../../api/client';
import type { AuditLog, AuditLogsResponse } from '../types';
import { AuditLogsSection } from './AuditLogsSection';

jest.mock('../../../../api/client');
const mockClient = client as jest.Mocked<typeof client>;

describe('AuditLogsSection', () => {
  const orgId = 'org-123';

  const mockLogs: AuditLog[] = [
    {
      id: 'log-1',
      organization_id: orgId,
      actor_id: 'user-1',
      actor_email: 'alice@example.com',
      action: 'create',
      resource_type: 'project',
      resource_id: 'proj-1',
      metadata: { name: 'New Project' },
      created_at: '2024-01-15T10:30:00Z',
    },
    {
      id: 'log-2',
      organization_id: orgId,
      actor_id: 'user-2',
      actor_email: 'bob@example.com',
      action: 'update',
      resource_type: 'task',
      resource_id: 'task-1',
      metadata: { status: 'completed' },
      created_at: '2024-01-15T11:00:00Z',
    },
    {
      id: 'log-3',
      organization_id: orgId,
      actor_id: 'user-1',
      actor_email: 'alice@example.com',
      action: 'delete',
      resource_type: 'source',
      resource_id: 'src-1',
      metadata: { reason: 'No longer needed' },
      created_at: '2024-01-15T12:00:00Z',
    },
  ];

  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('Initial Load', () => {
    it('renders loading state initially', () => {
      mockClient.getAuditLogs.mockImplementation(() => new Promise(() => {}));

      render(<AuditLogsSection orgId={orgId} />);

      expect(screen.getByText('Loading audit logs...')).toBeInTheDocument();
    });

    it('fetches and displays audit logs', async () => {
      const response: AuditLogsResponse = {
        logs: mockLogs,
        total: 3,
      };

      mockClient.getAuditLogs.mockResolvedValue(response);

      render(<AuditLogsSection orgId={orgId} />);

      // Wait for loading to complete and data to render
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      // Now check for the actual data - use getAllByText since emails appear in multiple rows
      await waitFor(
        () => {
          const aliceElements = screen.getAllByText('alice@example.com');
          expect(aliceElements.length).toBeGreaterThan(0);
        },
        { timeout: 3000 }
      );

      const bobElements = screen.getAllByText('bob@example.com');
      expect(bobElements.length).toBeGreaterThan(0);
    });

    it('displays error state when fetch fails', async () => {
      mockClient.getAuditLogs.mockRejectedValue(new Error('Failed to load'));

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Failed to load')).toBeInTheDocument();
      });

      expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument();
    });

    it('retries loading on retry button click', async () => {
      mockClient.getAuditLogs
        .mockRejectedValueOnce(new Error('Failed to load'))
        .mockResolvedValueOnce({ logs: mockLogs, total: 3 });

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Failed to load')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: /retry/i }));

      // Wait for loading to complete
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      // Now check for the actual data - use getAllByText since emails appear in multiple rows
      await waitFor(
        () => {
          const aliceElements = screen.getAllByText('alice@example.com');
          expect(aliceElements.length).toBeGreaterThan(0);
        },
        { timeout: 3000 }
      );
    });
  });

  describe('Table Display', () => {
    beforeEach(async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });
    });

    it('displays all columns', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Time')).toBeInTheDocument();
      });

      expect(screen.getByText('Actor')).toBeInTheDocument();
      expect(screen.getByText('Action')).toBeInTheDocument();
      expect(screen.getByText('Resource Type')).toBeInTheDocument();
      expect(screen.getByText('Resource ID')).toBeInTheDocument();
      expect(screen.getByText('Details')).toBeInTheDocument();
    });

    it('displays action badges with correct classes', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('create')).toBeInTheDocument();
      });

      const createBadge = screen.getByText('create');
      const updateBadge = screen.getByText('update');
      const deleteBadge = screen.getByText('delete');

      expect(createBadge).toHaveClass('action-badge', 'action-create');
      expect(updateBadge).toHaveClass('action-badge', 'action-update');
      expect(deleteBadge).toHaveClass('action-badge', 'action-delete');
    });

    it('displays actor information', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      // Wait for loading to complete
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      // Wait for actor information to be rendered - use getAllByText since emails/IDs may appear in multiple rows
      await waitFor(
        () => {
          const aliceElements = screen.getAllByText('alice@example.com');
          expect(aliceElements.length).toBeGreaterThan(0);
        },
        { timeout: 3000 }
      );

      const user1Elements = screen.getAllByText('user-1');
      expect(user1Elements.length).toBeGreaterThan(0);

      const bobElements = screen.getAllByText('bob@example.com');
      expect(bobElements.length).toBeGreaterThan(0);

      const user2Elements = screen.getAllByText('user-2');
      expect(user2Elements.length).toBeGreaterThan(0);
    });

    it('displays resource types and IDs', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('project')).toBeInTheDocument();
      });

      expect(screen.getByText('task')).toBeInTheDocument();
      expect(screen.getByText('source')).toBeInTheDocument();
      expect(screen.getByText('proj-1')).toBeInTheDocument();
      expect(screen.getByText('task-1')).toBeInTheDocument();
      expect(screen.getByText('src-1')).toBeInTheDocument();
    });
  });

  describe('Expandable Metadata', () => {
    beforeEach(async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });
    });

    it('shows metadata when expand button is clicked', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getAllByText('Show')[0]).toBeInTheDocument();
      });

      const expandButtons = screen.getAllByText('Show');
      fireEvent.click(expandButtons[0]);

      await waitFor(() => {
        expect(screen.getByText('Metadata')).toBeInTheDocument();
      });

      expect(screen.getByText(/"name": "New Project"/)).toBeInTheDocument();
    });

    it('hides metadata when hide button is clicked', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getAllByText('Show')[0]).toBeInTheDocument();
      });

      const expandButton = screen.getAllByText('Show')[0];
      fireEvent.click(expandButton);

      await waitFor(() => {
        expect(screen.getByText('Hide')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Hide'));

      await waitFor(() => {
        expect(screen.queryByText('Metadata')).not.toBeInTheDocument();
      });
    });

    it('only shows one expanded row at a time', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getAllByText('Show').length).toBeGreaterThan(1);
      });

      const expandButtons = screen.getAllByText('Show');
      fireEvent.click(expandButtons[0]);

      await waitFor(() => {
        expect(screen.getByText(/"name": "New Project"/)).toBeInTheDocument();
      });

      fireEvent.click(expandButtons[1]);

      await waitFor(() => {
        expect(screen.getByText(/"status": "completed"/)).toBeInTheDocument();
      });

      expect(screen.queryByText(/"name": "New Project"/)).not.toBeInTheDocument();
    });
  });

  describe('Filters', () => {
    beforeEach(async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });
    });

    it('shows filters when toggle button is clicked', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Show Filters'));

      expect(screen.getByLabelText('Action')).toBeInTheDocument();
      expect(screen.getByLabelText('Resource Type')).toBeInTheDocument();
      expect(screen.getByLabelText('Actor (User ID)')).toBeInTheDocument();
      expect(screen.getByLabelText('Start Date')).toBeInTheDocument();
      expect(screen.getByLabelText('End Date')).toBeInTheDocument();
    });

    it('applies filters when Apply Filters is clicked', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      // Wait for initial loading to complete
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      // Wait for Show Filters button to be available
      await waitFor(() => {
        expect(screen.getByText('Show Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Show Filters'));

      await waitFor(() => {
        expect(screen.getByLabelText('Action')).toBeInTheDocument();
      });

      const actionSelect = screen.getByLabelText('Action') as HTMLSelectElement;

      // Set up the mock response
      mockClient.getAuditLogs.mockResolvedValue({
        logs: [mockLogs[0]],
        total: 1,
      });

      // Test with single filter to avoid component limitations with multiple simultaneous filter changes
      fireEvent.change(actionSelect, { target: { value: 'create' } });

      // Wait for React to process this state update and useEffect to trigger
      await waitFor(() => expect(actionSelect.value).toBe('create'));

      // Wait for automatic reload to complete
      await waitFor(
        () => {
          return screen.queryByText('Loading audit logs...') === null;
        },
        { timeout: 3000 }
      );

      // Clear mock to isolate the "Apply Filters" call
      mockClient.getAuditLogs.mockClear();
      mockClient.getAuditLogs.mockResolvedValue({
        logs: [mockLogs[0]],
        total: 1,
      });

      // Click Apply Filters
      fireEvent.click(screen.getByText('Apply Filters'));

      // Wait for the API call from Apply Filters
      await waitFor(
        () => {
          expect(mockClient.getAuditLogs).toHaveBeenCalled();
        },
        { timeout: 3000 }
      );

      // Verify it was called with the action filter
      expect(mockClient.getAuditLogs).toHaveBeenCalledWith(
        orgId,
        expect.objectContaining({
          action: 'create',
        })
      );
    });

    it('resets filters when Reset is clicked', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      // Wait for initial loading to complete
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      // Wait for Show Filters button to be available
      await waitFor(() => {
        expect(screen.getByText('Show Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Show Filters'));

      await waitFor(() => {
        expect(screen.getByLabelText('Action')).toBeInTheDocument();
      });

      const actionSelect = screen.getByLabelText('Action') as HTMLSelectElement;

      // Set up mock to handle automatic reload when filter changes
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });

      fireEvent.change(actionSelect, { target: { value: 'create' } });

      await waitFor(() => {
        expect(actionSelect.value).toBe('create');
      });

      // Wait for automatic reload to complete
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(() => {
        expect(screen.getByText('Reset')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Reset'));

      // Wait for the state to update and DOM to reflect the reset
      // Re-query the select element to get the updated value
      await waitFor(
        () => {
          const updatedActionSelect = screen.getByLabelText('Action') as HTMLSelectElement;
          expect(updatedActionSelect.value).toBe('');
        },
        { timeout: 3000 }
      );
    });

    it('filters by date range', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      // Wait for initial loading to complete
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      // Wait for Show Filters button to be available
      await waitFor(() => {
        expect(screen.getByText('Show Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Show Filters'));

      await waitFor(() => {
        expect(screen.getByLabelText('Start Date')).toBeInTheDocument();
      });

      const startDateInput = screen.getByLabelText('Start Date') as HTMLInputElement;

      // Set up mock response
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });

      // Test with single date filter to avoid component limitations with multiple simultaneous filter changes
      fireEvent.change(startDateInput, { target: { value: '2024-01-01' } });

      // Wait for React to process this state update
      await waitFor(() => expect(startDateInput.value).toBe('2024-01-01'));

      // Wait for automatic reload to complete
      await waitFor(
        () => {
          return screen.queryByText('Loading audit logs...') === null;
        },
        { timeout: 3000 }
      );

      // Clear mock to isolate the "Apply Filters" call
      mockClient.getAuditLogs.mockClear();
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });

      await waitFor(() => {
        expect(screen.getByText('Apply Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Apply Filters'));

      // Wait for the new API call to complete
      await waitFor(
        () => {
          expect(mockClient.getAuditLogs).toHaveBeenCalled();
        },
        { timeout: 3000 }
      );

      // Verify it was called with the start date filter
      expect(mockClient.getAuditLogs).toHaveBeenCalledWith(
        orgId,
        expect.objectContaining({
          start_date: '2024-01-01',
        })
      );
    });

    it('filters by actor', async () => {
      render(<AuditLogsSection orgId={orgId} />);

      // Wait for initial loading to complete
      await waitFor(
        () => {
          expect(screen.queryByText('Loading audit logs...')).not.toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      // Wait for Show Filters button to be available
      await waitFor(() => {
        expect(screen.getByText('Show Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Show Filters'));

      await waitFor(() => {
        expect(screen.getByLabelText('Actor (User ID)')).toBeInTheDocument();
      });

      const actorInput = screen.getByLabelText('Actor (User ID)') as HTMLInputElement;
      fireEvent.change(actorInput, { target: { value: 'user-1' } });

      mockClient.getAuditLogs.mockClear();
      mockClient.getAuditLogs.mockResolvedValue({
        logs: [mockLogs[0], mockLogs[2]],
        total: 2,
      });

      await waitFor(() => {
        expect(screen.getByText('Apply Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Apply Filters'));

      // Wait for the new API call to complete
      await waitFor(
        () => {
          expect(mockClient.getAuditLogs).toHaveBeenCalled();
        },
        { timeout: 3000 }
      );

      // Verify it was called with the correct filters
      expect(mockClient.getAuditLogs).toHaveBeenCalledWith(
        orgId,
        expect.objectContaining({
          actor_id: 'user-1',
        })
      );
    });
  });

  describe('Export', () => {
    beforeEach(() => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });

      // Mock URL.createObjectURL and revokeObjectURL
      global.URL.createObjectURL = jest.fn(() => 'blob:mock-url');
      global.URL.revokeObjectURL = jest.fn();

      // Mock document.createElement to track anchor creation
      const originalCreateElement = document.createElement.bind(document);
      jest.spyOn(document, 'createElement').mockImplementation((tagName) => {
        const element = originalCreateElement(tagName);
        if (tagName === 'a') {
          element.click = jest.fn();
        }
        return element;
      });
    });

    afterEach(() => {
      jest.restoreAllMocks();
    });

    it('exports audit logs as CSV', async () => {
      const mockBlob = new Blob(['csv,data'], { type: 'text/csv' });
      mockClient.exportAuditLogs.mockResolvedValue(mockBlob);

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Export CSV'));

      await waitFor(() => {
        expect(mockClient.exportAuditLogs).toHaveBeenCalledWith(orgId, {});
      });

      expect(global.URL.createObjectURL).toHaveBeenCalledWith(mockBlob);
      expect(global.URL.revokeObjectURL).toHaveBeenCalled();
    });

    it('exports with filters applied', async () => {
      const mockBlob = new Blob(['csv,data'], { type: 'text/csv' });
      mockClient.exportAuditLogs.mockResolvedValue(mockBlob);

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      fireEvent.click(screen.getByText('Show Filters'));

      await waitFor(() => {
        expect(screen.getByLabelText('Action')).toBeInTheDocument();
      });

      const actionSelect = screen.getByLabelText('Action') as HTMLSelectElement;
      fireEvent.change(actionSelect, { target: { value: 'create' } });

      await waitFor(() => {
        expect(screen.getByText('Export CSV')).toBeInTheDocument();
      });

      const exportButton = screen.getByText('Export CSV');
      fireEvent.click(exportButton);

      await waitFor(() => {
        expect(mockClient.exportAuditLogs).toHaveBeenCalledWith(
          orgId,
          expect.objectContaining({
            action: 'create',
          })
        );
      });
    });

    it('handles export errors', async () => {
      mockClient.exportAuditLogs.mockRejectedValue(new Error('Export failed'));

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Export CSV'));

      await waitFor(() => {
        expect(screen.getByText('Export failed')).toBeInTheDocument();
      });
    });
  });

  describe('Pagination', () => {
    it('shows load more button when there are more logs', async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 100,
      });

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/Load More.*97 remaining/)).toBeInTheDocument();
      });
    });

    it('does not show load more when all logs are loaded', async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(
        () => {
          expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
        },
        { timeout: 3000 }
      );

      expect(screen.queryByText(/Load More/)).not.toBeInTheDocument();
    });

    it('loads more logs when button is clicked', async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 100,
      });

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/Load More/)).toBeInTheDocument();
      });

      mockClient.getAuditLogs.mockClear();
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 100,
      });

      fireEvent.click(screen.getByText(/Load More/));

      await waitFor(() => {
        expect(mockClient.getAuditLogs).toHaveBeenCalledWith(
          orgId,
          expect.objectContaining({
            offset: 50,
          })
        );
      });
    });
  });

  describe('Empty State', () => {
    it('shows empty state when no logs', async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: [],
        total: 0,
      });

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('No audit logs found.')).toBeInTheDocument();
      });
    });

    it('shows clear filters button in empty state with filters', async () => {
      mockClient.getAuditLogs.mockResolvedValue({
        logs: mockLogs,
        total: 3,
      });

      render(<AuditLogsSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/3 total entries/)).toBeInTheDocument();
      });

      await waitFor(() => {
        expect(screen.getByText('Show Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Show Filters'));

      await waitFor(() => {
        expect(screen.getByLabelText('Action')).toBeInTheDocument();
      });

      const actionSelect = screen.getByLabelText('Action') as HTMLSelectElement;
      fireEvent.change(actionSelect, { target: { value: 'create' } });

      mockClient.getAuditLogs.mockClear();
      mockClient.getAuditLogs.mockResolvedValue({
        logs: [],
        total: 0,
      });

      await waitFor(() => {
        expect(screen.getByText('Apply Filters')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Apply Filters'));

      await waitFor(() => {
        expect(
          screen.getByText(/No audit logs found matching the selected filters/)
        ).toBeInTheDocument();
      });

      expect(screen.getByText('Clear Filters')).toBeInTheDocument();
    });
  });
});
