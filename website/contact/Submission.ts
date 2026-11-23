export interface Submission {
  readonly name: string;
  readonly email: string;
  readonly company: string;
  readonly message: string;
  readonly redirect: URL;
}
