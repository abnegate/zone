#!/usr/bin/env python3
"""Check the Prometheus scrape config and Grafana alert rules with PyYAML and promtool."""

import re
import shutil
import subprocess
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

import yaml

ROOT = Path(__file__).resolve().parents[1]
PROMETHEUS_CONFIG = ROOT / 'prometheus' / 'prometheus.yml'
ALERT_RULES = ROOT / 'grafana' / 'provisioning' / 'alerting' / 'rules.yml'
COMPOSE = ROOT / 'docker-compose.yml'


def load(path: Path) -> dict:
    return yaml.safe_load(path.read_text())


def rules() -> dict[str, dict]:
    return {
        rule['uid']: rule
        for group in load(ALERT_RULES)['groups']
        for rule in group['rules']
    }


def prometheus_queries(rule: dict) -> list[str]:
    return [
        query['model']['expr']
        for query in rule['data']
        if query['datasourceUid'] == 'prometheus'
    ]


def prometheus_image() -> str:
    match = re.search(
        r'prom/prometheus:\$\{DOCKER_VERSION_PROMETHEUS:-([^}]+)\}'
        r'@\$\{DOCKER_DIGEST_PROMETHEUS:-(sha256:[0-9a-f]{64})\}',
        COMPOSE.read_text(),
    )
    if match is None:
        raise AssertionError('docker-compose.yml does not pin prom/prometheus by version and digest')
    return f'prom/prometheus:{match.group(1)}@{match.group(2)}'


def docker_available() -> bool:
    if shutil.which('docker') is None:
        return False
    try:
        result = subprocess.run(['docker', 'info'], capture_output=True, timeout=30)
    except subprocess.TimeoutExpired:
        return False
    return result.returncode == 0


class BudgetRuleTests(unittest.TestCase):
    def test_unlimited_keys_are_excluded_from_the_minimum(self) -> None:
        queries = prometheus_queries(rules()['litellm-budget-low'])
        self.assertEqual(len(queries), 1)
        self.assertIn('< +Inf', queries[0], 'an unlimited key reports +Inf, which replaceNN turns into 0')

    def test_no_data_is_ok(self) -> None:
        self.assertEqual(rules()['litellm-budget-low']['noDataState'], 'OK')


@unittest.skipUnless(docker_available(), 'docker is not available')
class PromtoolTests(unittest.TestCase):
    def promtool(self, folder: Path, *arguments: str) -> None:
        result = subprocess.run(
            ['docker', 'run', '--rm', '--network', 'none', '--entrypoint', 'promtool',
             '--volume', f'{folder}:/check:ro', prometheus_image(), *arguments],
            capture_output=True, text=True, timeout=300,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_scrape_config_is_valid(self) -> None:
        self.promtool(PROMETHEUS_CONFIG.parent, 'check', 'config', '--syntax-only', '/check/prometheus.yml')

    def test_rule_queries_are_valid(self) -> None:
        queries = [
            {'alert': f'{uid}-{index}', 'expr': expression}
            for uid, rule in rules().items()
            for index, expression in enumerate(prometheus_queries(rule))
        ]
        self.assertTrue(queries)
        with TemporaryDirectory() as folder:
            document = {'groups': [{'name': 'grafana', 'rules': queries}]}
            (Path(folder) / 'rules.yml').write_text(yaml.safe_dump(document))
            self.promtool(Path(folder), 'check', 'rules', '/check/rules.yml')


if __name__ == '__main__':
    unittest.main()
