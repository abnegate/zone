#!/usr/bin/env python3
"""Exercise the manager's actual Cargo RUN with persistent cache and old sources."""

import os
import re
import subprocess
import tempfile
import uuid
from pathlib import Path


def main() -> None:
    dockerfile = (
        Path(__file__).resolve().parents[1] / 'manager/Dockerfile'
    ).read_text()
    image = re.search(r'^FROM (\S+) AS builder$', dockerfile, re.MULTILINE).group(1)
    command = re.search(
        r'^RUN --mount=type=cache.*?(?=\n\n)',
        dockerfile,
        re.MULTILINE | re.DOTALL,
    ).group(0)
    command = command.replace('zone-manager-', f'zone-cache-test-{uuid.uuid4().hex}-')
    with tempfile.TemporaryDirectory(prefix='zone-cache-test-') as directory:
        context = Path(directory) / 'context'
        context.mkdir()
        dependency = context / 'dependency'
        dependency.mkdir()
        (dependency / 'Cargo.toml').write_text(
            '[package]\nname="fixture_dependency"\nversion="0.1.0"\nedition="2024"\n[lib]\npath="lib.rs"\n'
        )
        # Unique source prevents Docker's layer cache from skipping the cold build.
        (dependency / 'lib.rs').write_text(
            'pub fn message() -> &\'static str { "dependency" }\n'
            f'// Unique fixture: {uuid.uuid4().hex}\n'
        )
        runner = context / 'runner'
        runner.mkdir()
        (runner / 'Cargo.toml').write_text(
            '[workspace]\nresolver="2"\nmembers=["zone_server", "zone_runner"]\n'
        )
        for name in ('zone_server', 'zone_runner'):
            crate = runner / name
            crate.mkdir()
            (crate / 'Cargo.toml').write_text(
                f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n[[bin]]\nname="{name.replace("_", "-")}"\npath="main.rs"\n[dependencies]\nfixture_dependency={{path="../../dependency"}}\n'
            )
            (crate / 'main.rs').write_text(
                'fn main() { println!("first {}", fixture_dependency::message()); }\n'
            )
        (context / 'Dockerfile').write_text(f"""# syntax=docker/dockerfile:1
FROM {image} AS builder
WORKDIR /build/runner
ARG TARGETARCH
ARG TARGETVARIANT
COPY dependency /build/dependency
COPY runner /build/runner
RUN cargo generate-lockfile --offline
{command}
RUN /build/bin/zone-server > /build/bin/result
FROM scratch
COPY --from=builder /build/bin/ /
""")
        logs = []
        for iteration, expected in enumerate(('first dependency', 'second dependency')):
            if iteration:
                source = runner / 'zone_server/main.rs'
                source.write_text(
                    'fn main() { println!("second {}", fixture_dependency::message()); }\n'
                )
                os.utime(source, (1, 1))
            output = Path(directory) / f'output-{iteration}'
            result = subprocess.run(
                [
                    'docker',
                    'buildx',
                    'build',
                    '--progress=plain',
                    '--output',
                    f'type=local,dest={output}',
                    str(context),
                ],
                text=True,
                capture_output=True,
                check=True,
            )
            logs.append(result.stderr + result.stdout)
            assert (output / 'result').read_text().strip() == expected, (
                'Stale binary exported'
            )
            assert (output / 'zone-runner').is_file(), 'Runner binary missing'
        assert 'Compiling fixture_dependency' in logs[0], (
            'Cold dependency build missing'
        )
        assert 'Compiling fixture_dependency' not in logs[1], (
            'Dependency rebuilt after source edit'
        )
        assert 'Compiling zone_server' in logs[1], (
            'Source edit did not trigger compilation'
        )
        print(
            'PASS: old-mtime source changed exported binary; dependency reused; both binaries exported'
        )


if __name__ == '__main__':
    main()
