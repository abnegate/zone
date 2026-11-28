import type { CLIBuild } from './CLIBuild.ts';
import type { DownloadGroup } from './DownloadGroup.ts';

export const desktopDownloads: ReadonlyArray<DownloadGroup> = [
  {
    id: 'macos',
    platform: 'macOS',
    note: 'Unzip it and move Zone.app to Applications. The zone CLI comes inside the app.',
    downloads: [
      { label: 'Apple silicon', format: '.zip', asset: 'Zone-darwin-arm64.zip' },
      { label: 'Intel', format: '.zip', asset: 'Zone-darwin-x86_64.zip' },
    ],
  },
  {
    id: 'debian',
    platform: 'Debian and Ubuntu',
    note: 'Installs the desktop app and the zone CLI. Open Zone from your applications, or run zone-desktop.',
    command: 'sudo apt install ./zone_*.deb',
    downloads: [
      { label: 'x86_64', format: '.deb', asset: 'zone_amd64.deb' },
      { label: 'ARM64', format: '.deb', asset: 'zone_arm64.deb' },
    ],
  },
  {
    id: 'linux',
    platform: 'Other Linux',
    note: 'Unpack it anywhere, then run zone-desktop from the bin folder inside. The zone CLI is beside it.',
    command: 'tar -xzf zone-*-linux-gnu.tar.gz',
    downloads: [
      { label: 'x86_64', format: '.tar.gz', asset: 'zone-x86_64-unknown-linux-gnu.tar.gz' },
      { label: 'ARM64', format: '.tar.gz', asset: 'zone-aarch64-unknown-linux-gnu.tar.gz' },
    ],
  },
];

export const cliBuilds: ReadonlyArray<CLIBuild> = [
  {
    id: 'macos-arm64',
    platform: 'macOS, Apple silicon',
    asset: 'zone-aarch64-apple-darwin',
    checksumCommand: 'shasum -a 256',
    installDirectoryMayBeMissing: true,
  },
  {
    id: 'macos-x86-64',
    platform: 'macOS, Intel',
    asset: 'zone-x86_64-apple-darwin',
    checksumCommand: 'shasum -a 256',
    installDirectoryMayBeMissing: true,
  },
  {
    id: 'linux-x86-64',
    platform: 'Linux, x86_64',
    asset: 'zone-x86_64-unknown-linux-gnu',
    checksumCommand: 'sha256sum',
    installDirectoryMayBeMissing: false,
  },
  {
    id: 'linux-arm64',
    platform: 'Linux, ARM64',
    asset: 'zone-aarch64-unknown-linux-gnu',
    checksumCommand: 'sha256sum',
    installDirectoryMayBeMissing: false,
  },
];
