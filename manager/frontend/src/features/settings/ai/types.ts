import type { AgentLogin, ClaudeScope, SignInFlow } from './schemas';

export type AgentAccess = 'manage' | 'view' | 'resolving';

export type SignInAction = 'start' | 'restart' | 'full' | 'paste' | 'submit' | 'cancel' | 'signOut';

export interface Attempt {
  login: AgentLogin;
  scope: ClaudeScope | undefined;
  /** The flow the admin asked for; the server's own choice when undefined. */
  flow: SignInFlow | undefined;
  spent: boolean;
}
