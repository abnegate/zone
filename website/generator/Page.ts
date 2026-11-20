export interface Page {
  readonly template: string;
  readonly output: string;
  readonly title: string;
  readonly description: string;
  readonly navigationLabel?: string;
  readonly indexed: boolean;
  readonly linksFromBasePath: boolean;
}
