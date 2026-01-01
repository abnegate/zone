import { render, screen } from '@testing-library/react';
import WikiPage from './WikiPage';

describe('WikiPage', () => {
  it('renders page heading', () => {
    render(<WikiPage />);
    expect(screen.getByRole('heading', { name: 'Wiki' })).toBeInTheDocument();
  });

  it('renders subtitle', () => {
    render(<WikiPage />);
    expect(screen.getByText('Knowledge base for your AI models')).toBeInTheDocument();
  });

  it('renders knowledge base heading', () => {
    render(<WikiPage />);
    expect(screen.getByRole('heading', { name: 'Knowledge Base' })).toBeInTheDocument();
  });

  it('renders feature list', () => {
    render(<WikiPage />);
    expect(screen.getByText('Auto-populated from chat conversations')).toBeInTheDocument();
    expect(screen.getByText('Import docs, links, and content')).toBeInTheDocument();
    expect(screen.getByText('Models learn from the knowledge base')).toBeInTheDocument();
  });
});
