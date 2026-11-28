export interface CLIBuild {
  readonly id: string;
  readonly platform: string;
  readonly asset: string;
  readonly checksumCommand: string;
  readonly installDirectoryMayBeMissing: boolean;
}
