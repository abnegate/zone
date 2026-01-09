// Re-export shared components from @zone/ui
export { Button, Input, Select, Checkbox, InfoBox, Modal, ProgressBar } from '@zone/ui';
export type {
  ButtonProps,
  InputProps,
  SelectProps,
  SelectOption,
  CheckboxProps,
  InfoBoxProps,
  ModalProps,
  ProgressBarProps,
} from '@zone/ui';

// Re-export shared components
export {
  ContextSwitcher,
  Layout,
  PermissionGate,
  ProtectedRoute,
  Sidebar,
} from '../shared/components';

// App-specific components - now re-exported from features/settings
export {
  AuditLogsSection,
  BillingSection,
  InvitationsSection,
  OrgMembersSection,
} from '../features/settings/organization/components';
export { WorkspaceMembersSection } from '../features/settings/workspace/components';

// Backward compatibility re-exports
export { VirtualBrowseList } from '../features/models/components';
export { ResendVerificationButton, VerificationPendingBanner } from '../features/auth';
