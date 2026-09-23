import type { AgentLogin, ClaudeScope } from './schemas';

export type SignInAction = 'start' | 'restart' | 'full' | 'submit' | 'signOut';

export interface Attempt {
  login: AgentLogin;
  scope: ClaudeScope | undefined;
  spent: boolean;
}
