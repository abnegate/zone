export interface Response {
  text(body: string, status?: number, headers?: Readonly<Record<string, string>>): unknown;
}

export interface Context {
  readonly req: {
    readonly method: string;
    readonly bodyText: string;
  };
  readonly res: Response;
  error(message: string): void;
}
