import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import type { Limits, Subscription, Usage } from '../types';

// Mock the client
const mockClient = {
  getSubscription: mock(),
  getUsage: mock(),
  getLimits: mock(),
};

mock.module('../../../../api/client', () => ({
  client: mockClient,
}));

let BillingSection: typeof import('./BillingSection').BillingSection;

beforeAll(async () => {
  ({ BillingSection } = await import('./BillingSection'));
});

afterAll(() => {
  mock.restore();
});

describe('BillingSection', () => {
  const orgId = 'org-123';

  const mockSubscription: Subscription = {
    id: 'sub-123',
    organization_id: orgId,
    plan_id: 'plan-pro',
    plan_name: 'Pro',
    status: 'active',
    current_period_start: '2024-01-01T00:00:00Z',
    current_period_end: '2024-02-01T00:00:00Z',
    cancel_at_period_end: false,
  };

  const mockUsage: Usage = {
    members: 15,
    workspaces: 3,
    chat_messages: 12543,
    period_start: '2024-01-01T00:00:00Z',
    period_end: '2024-02-01T00:00:00Z',
  };

  const mockLimits: Limits = {
    max_members: 50,
    max_workspaces: 10,
    max_chats_per_month: 50000,
  };

  beforeEach(() => {
    mock.clearAllMocks();
  });

  describe('Loading State', () => {
    it('displays loading message while fetching data', () => {
      mockClient.getSubscription.mockImplementation(
        () => new Promise(() => {}) // Never resolves
      );
      mockClient.getUsage.mockImplementation(() => new Promise(() => {}));
      mockClient.getLimits.mockImplementation(() => new Promise(() => {}));

      render(<BillingSection orgId={orgId} />);

      expect(screen.getByText('Loading billing information...')).toBeInTheDocument();
    });
  });

  describe('Error State', () => {
    it('displays error message when API call fails', async () => {
      mockClient.getSubscription.mockRejectedValue(new Error('Failed to fetch subscription'));
      mockClient.getUsage.mockResolvedValue(mockUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Failed to fetch subscription')).toBeInTheDocument();
      });
    });

    it('displays retry button on error', async () => {
      mockClient.getSubscription.mockRejectedValue(new Error('Network error'));
      mockClient.getUsage.mockResolvedValue(mockUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument();
      });
    });
  });

  describe('Subscription Display', () => {
    beforeEach(() => {
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(mockUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);
    });

    it('displays subscription plan name', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Pro')).toBeInTheDocument();
      });
    });

    it('displays the default plan of an organization that was never billed', async () => {
      mockClient.getSubscription.mockResolvedValue({
        ...mockSubscription,
        plan_id: 'plan-free',
        plan_name: 'Free',
      });
      mockClient.getUsage.mockResolvedValue({
        ...mockUsage,
        members: 1,
        workspaces: 1,
        chat_messages: 0,
      });
      mockClient.getLimits.mockResolvedValue({
        max_members: 3,
        max_workspaces: 1,
        max_chats_per_month: 100,
      });

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Free')).toBeInTheDocument();
        expect(screen.getByText(/3 members/)).toBeInTheDocument();
        expect(screen.getByText(/100 this period/)).toBeInTheDocument();
      });
      expect(screen.queryByText(/No subscription found/)).not.toBeInTheDocument();
    });

    it('displays subscription status badge', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        const statusBadge = screen.getByText('active');
        expect(statusBadge).toBeInTheDocument();
        expect(statusBadge).toHaveClass('status-active');
      });
    });

    it('displays billing period dates', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        const jan1Elements = screen.getAllByText(/January 1, 2024/);
        expect(jan1Elements.length).toBeGreaterThan(0);
        const feb1Elements = screen.getAllByText(/February 1, 2024/);
        expect(feb1Elements.length).toBeGreaterThan(0);
      });
    });

    it('displays cancellation warning when cancel_at_period_end is true', async () => {
      const cancelingSubscription = { ...mockSubscription, cancel_at_period_end: true };
      mockClient.getSubscription.mockResolvedValue(cancelingSubscription);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(
          screen.getByText(/Subscription will be canceled at the end of the current billing period/)
        ).toBeInTheDocument();
      });
    });

    it('does not display cancellation warning when cancel_at_period_end is false', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.queryByText(/will be canceled/)).not.toBeInTheDocument();
      });
    });

    it('displays different status badge colors', async () => {
      const statuses: Array<Subscription['status']> = [
        'active',
        'trialing',
        'past_due',
        'canceled',
      ];
      const statusClasses = [
        'status-active',
        'status-trialing',
        'status-past-due',
        'status-canceled',
      ];

      for (let i = 0; i < statuses.length; i++) {
        const subscription = { ...mockSubscription, status: statuses[i] };
        mockClient.getSubscription.mockResolvedValue(subscription);

        const { unmount } = render(<BillingSection orgId={orgId} />);

        await waitFor(() => {
          const badge = screen.getByText(statuses[i]);
          expect(badge).toHaveClass(statusClasses[i]);
        });

        unmount();
      }
    });
  });

  describe('Usage Metrics', () => {
    beforeEach(() => {
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(mockUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);
    });

    it('displays the metered usage metrics', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Members')).toBeInTheDocument();
        expect(screen.getByText('Workspaces')).toBeInTheDocument();
        expect(screen.getByText('Chat Messages')).toBeInTheDocument();
      });
    });

    it('displays current usage values', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('15')).toBeInTheDocument();
        expect(screen.getByText('3')).toBeInTheDocument();
        expect(screen.getByText(/12,543/)).toBeInTheDocument();
      });
    });

    it('displays formatted limit values', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/50 members/)).toBeInTheDocument();
        expect(screen.getByText(/10 workspaces/)).toBeInTheDocument();
        expect(screen.getByText(/50,000 this period/)).toBeInTheDocument();
      });
    });

    it('displays percentage used for each metric', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        const percentageElements = document.querySelectorAll('.percentage-label');
        const percentages = Array.from(percentageElements).map((el) => el.textContent);
        expect(percentages).toContain('30.0% used');
        expect(percentages).toContain('25.1% used');
      });
    });

    it('displays progress bars with correct widths', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        const progressBars = screen.getAllByRole('progressbar');
        expect(progressBars.length).toBeGreaterThan(0);
        expect(progressBars[0]).toHaveStyle({ width: '30%' });
      });
    });

    it('displays unlimited label for null limits', async () => {
      const unlimitedLimits: Limits = {
        max_members: null,
        max_workspaces: null,
        max_chats_per_month: 100000,
      };
      mockClient.getLimits.mockResolvedValue(unlimitedLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        const unlimitedLabels = screen.getAllByText('(unlimited)');
        expect(unlimitedLabels).toHaveLength(2);
      });
    });
  });

  describe('Warning Indicators', () => {
    it('displays warning badge when usage is above 80%', async () => {
      const highUsage: Usage = {
        ...mockUsage,
        members: 45,
      };
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(highUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Near Limit')).toBeInTheDocument();
      });
    });

    it('displays critical badge when usage is above 95%', async () => {
      const criticalUsage: Usage = {
        ...mockUsage,
        members: 49,
      };
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(criticalUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText('Limit Reached')).toBeInTheDocument();
      });
    });

    it('applies correct progress bar color for different usage levels', async () => {
      const testCases = [
        { members: 25, expectedClass: 'ok' },
        { members: 42, expectedClass: 'warning' },
        { members: 49, expectedClass: 'critical' },
      ];

      for (const testCase of testCases) {
        const usage: Usage = { ...mockUsage, members: testCase.members };
        mockClient.getSubscription.mockResolvedValue(mockSubscription);
        mockClient.getUsage.mockResolvedValue(usage);
        mockClient.getLimits.mockResolvedValue(mockLimits);

        const { unmount } = render(<BillingSection orgId={orgId} />);

        await waitFor(() => {
          const progressFill = document.querySelector('.progress-fill');
          expect(progressFill).toHaveClass(testCase.expectedClass);
        });

        unmount();
      }
    });
  });

  describe('Formatting', () => {
    beforeEach(() => {
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(mockUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);
    });

    it('formats large numbers with commas', async () => {
      const largeUsage: Usage = {
        ...mockUsage,
        chat_messages: 1234567,
      };
      mockClient.getUsage.mockResolvedValue(largeUsage);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/1,234,567/)).toBeInTheDocument();
      });
    });

    it('formats dates in readable format', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getAllByText(/January 1, 2024/).length).toBeGreaterThan(0);
      });
    });

    it('displays usage period dates', async () => {
      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getByText(/Current period:/)).toBeInTheDocument();
      });
    });
  });

  describe('API Calls', () => {
    it('fetches all billing data on mount', async () => {
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(mockUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(mockClient.getSubscription).toHaveBeenCalledWith(orgId);
        expect(mockClient.getUsage).toHaveBeenCalledWith(orgId);
        expect(mockClient.getLimits).toHaveBeenCalledWith(orgId);
      });
    });

    it('fetches data in parallel', async () => {
      let getSubscriptionCalled = false;
      let getUsageCalled = false;
      let getLimitsCalled = false;

      mockClient.getSubscription.mockImplementation(async () => {
        getSubscriptionCalled = true;
        await new Promise((resolve) => setTimeout(resolve, 100));
        return mockSubscription;
      });

      mockClient.getUsage.mockImplementation(async () => {
        getUsageCalled = true;
        await new Promise((resolve) => setTimeout(resolve, 100));
        return mockUsage;
      });

      mockClient.getLimits.mockImplementation(async () => {
        getLimitsCalled = true;
        await new Promise((resolve) => setTimeout(resolve, 100));
        return mockLimits;
      });

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(getSubscriptionCalled).toBe(true);
        expect(getUsageCalled).toBe(true);
        expect(getLimitsCalled).toBe(true);
      });
    });
  });

  describe('Edge Cases', () => {
    it('handles zero usage values', async () => {
      const zeroUsage: Usage = {
        members: 0,
        workspaces: 0,
        chat_messages: 0,
        period_start: '2024-01-01T00:00:00Z',
        period_end: '2024-02-01T00:00:00Z',
      };
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(zeroUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        const percentageElements = document.querySelectorAll('.percentage-label');
        const percentages = Array.from(percentageElements).map((el) => el.textContent);
        expect(percentages).toContain('0.0% used');
      });
    });

    it('handles usage at exactly 100%', async () => {
      const fullUsage: Usage = {
        ...mockUsage,
        members: 50,
      };
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(fullUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        const percentageElements = document.querySelectorAll('.percentage-label');
        const percentages = Array.from(percentageElements).map((el) => el.textContent);
        expect(percentages).toContain('100.0% used');
      });
    });

    it('caps percentage at 100% for over-limit usage', async () => {
      const overUsage: Usage = {
        ...mockUsage,
        members: 60,
      };
      mockClient.getSubscription.mockResolvedValue(mockSubscription);
      mockClient.getUsage.mockResolvedValue(overUsage);
      mockClient.getLimits.mockResolvedValue(mockLimits);

      render(<BillingSection orgId={orgId} />);

      await waitFor(() => {
        expect(screen.getAllByRole('progressbar')[0]).toBeInTheDocument();
      });
      expect(screen.getAllByRole('progressbar')[0]).toHaveStyle({ width: '100%' });
    });
  });
});
