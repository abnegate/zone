export const GitHubConnectionMode = {
  App: 'app',
  Token: 'token',
} as const;

export type GitHubConnectionMode = (typeof GitHubConnectionMode)[keyof typeof GitHubConnectionMode];
