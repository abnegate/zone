import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { FieldSource } from '../inheritance';
import InheritedField from './InheritedField';

type Props = Parameters<typeof InheritedField>[0];

function renderField(changes: Partial<Props> = {}) {
  const onOverride = mock(() => {});
  const onReset = mock(() => {});
  const props: Props = {
    label: 'Fast Model',
    value: 'gpt-4o-mini',
    source: FieldSource.Organization,
    editable: true,
    editing: false,
    busy: false,
    onOverride,
    onReset,
    ...changes,
  };
  const { container, unmount } = render(<InheritedField {...props} />);
  return {
    container,
    unmount,
    group: screen.getByRole('group', { name: props.label }),
    onOverride: props.onOverride as typeof onOverride,
    onReset: props.onReset as typeof onReset,
  };
}

describe('InheritedField', () => {
  it('renders a labelled group holding the label and the value', () => {
    const { container, group } = renderField();

    expect(group).toHaveClass('inherited-field');
    expect(container.querySelector('div.inherited-field[role="group"]')).toBe(group);
    expect(group).toHaveAttribute('aria-label', 'Fast Model');
    expect(within(group).getByText('Fast Model')).toBeInTheDocument();
    expect(within(group).getByText('gpt-4o-mini')).toBeInTheDocument();
  });

  it('badges a value inherited from the organization', () => {
    const { group } = renderField({ source: FieldSource.Organization });

    expect(within(group).getByText('Inherited from organization')).toBeInTheDocument();
    expect(within(group).queryByText('Set for this workspace')).not.toBeInTheDocument();
  });

  it('badges a value set for this workspace', () => {
    const { group } = renderField({ source: FieldSource.Workspace });

    expect(within(group).getByText('Set for this workspace')).toBeInTheDocument();
    expect(within(group).queryByText('Inherited from organization')).not.toBeInTheDocument();
  });

  it('offers only Override on an inherited value', () => {
    const { group, onOverride, onReset } = renderField({ source: FieldSource.Organization });

    const override = within(group).getByRole('button', { name: 'Override Fast Model' });
    expect(override).toHaveTextContent('Override');
    expect(within(group).getAllByRole('button')).toEqual([override]);

    fireEvent.click(override);
    expect(onOverride).toHaveBeenCalledTimes(1);
    expect(onReset).not.toHaveBeenCalled();
  });

  it('offers Edit and Reset on a workspace value', () => {
    const { group, onOverride, onReset } = renderField({ source: FieldSource.Workspace });

    const edit = within(group).getByRole('button', { name: 'Edit Fast Model' });
    const reset = within(group).getByRole('button', { name: 'Reset Fast Model' });
    expect(edit).toHaveTextContent('Edit');
    expect(reset).toHaveTextContent('Reset');
    expect(within(group).getAllByRole('button')).toHaveLength(2);
    expect(
      within(group).queryByRole('button', { name: 'Override Fast Model' })
    ).not.toBeInTheDocument();

    fireEvent.click(edit);
    expect(onOverride).toHaveBeenCalledTimes(1);
    fireEvent.click(reset);
    expect(onReset).toHaveBeenCalledTimes(1);
  });

  it('names the buttons after the field label', () => {
    renderField({ label: 'Video Model', source: FieldSource.Workspace });

    expect(screen.getByRole('button', { name: 'Edit Video Model' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Reset Video Model' })).toBeInTheDocument();
  });

  it('disables Override while busy', () => {
    const { group, onOverride } = renderField({ source: FieldSource.Organization, busy: true });

    const override = within(group).getByRole('button', { name: 'Override Fast Model' });
    expect(override).toBeDisabled();
    fireEvent.click(override);
    expect(onOverride).not.toHaveBeenCalled();
  });

  it('disables Edit and Reset while busy', () => {
    const { group, onOverride, onReset } = renderField({
      source: FieldSource.Workspace,
      busy: true,
    });

    const edit = within(group).getByRole('button', { name: 'Edit Fast Model' });
    const reset = within(group).getByRole('button', { name: 'Reset Fast Model' });
    expect(edit).toBeDisabled();
    expect(reset).toBeDisabled();
    fireEvent.click(edit);
    fireEvent.click(reset);
    expect(onOverride).not.toHaveBeenCalled();
    expect(onReset).not.toHaveBeenCalled();
  });

  it('renders the editor children in place of the buttons while editing', () => {
    const { group } = renderField({
      source: FieldSource.Workspace,
      editing: true,
      children: <select aria-label="Fast Model editor" />,
    });

    expect(within(group).getByRole('combobox', { name: 'Fast Model editor' })).toBeInTheDocument();
    expect(
      within(group).queryByRole('button', { name: 'Edit Fast Model' })
    ).not.toBeInTheDocument();
    expect(
      within(group).queryByRole('button', { name: 'Reset Fast Model' })
    ).not.toBeInTheDocument();
  });

  it('keeps the editor children hidden until editing', () => {
    const { group } = renderField({ children: <select aria-label="Fast Model editor" /> });

    expect(
      within(group).queryByRole('combobox', { name: 'Fast Model editor' })
    ).not.toBeInTheDocument();
  });

  it('shows no buttons when the field is not editable', () => {
    for (const source of [FieldSource.Organization, FieldSource.Workspace]) {
      const { group, unmount } = renderField({ source, editable: false });

      expect(within(group).queryAllByRole('button')).toHaveLength(0);
      expect(within(group).getByText('gpt-4o-mini')).toBeInTheDocument();
      unmount();
    }
  });

  it('keeps the source badge when the field is not editable', () => {
    const { group } = renderField({ source: FieldSource.Workspace, editable: false });

    expect(within(group).getByText('Set for this workspace')).toBeInTheDocument();
  });
});
