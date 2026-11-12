export class ActiveRunError extends Error {
  constructor(
    message: string,
    readonly ids: readonly string[]
  ) {
    super(message);
    this.name = 'ActiveRunError';
  }
}
