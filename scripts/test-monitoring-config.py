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
GLUETUN_METRICS = 'gluetun:8001'
SEARXNG_TARGET = 'http://gluetun:8080/'
BUDGET_RECORD = 'litellm_budget_low:minimum'


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


def jobs() -> dict[str, dict]:
    return {job['job_name']: job for job in load(PROMETHEUS_CONFIG)['scrape_configs']}


def relabelled(job: dict, label: str) -> list[str]:
    return [
        rule['replacement']
        for rule in job.get('relabel_configs', [])
        if rule.get('target_label') == label and 'replacement' in rule
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
    def test_non_numeric_budgets_are_dropped(self) -> None:
        reductions = [
            query['model']
            for query in rules()['litellm-budget-low']['data']
            if query['model'].get('type') == 'reduce'
        ]
        self.assertEqual(len(reductions), 1)
        self.assertEqual(reductions[0]['settings'], {'mode': 'dropNN'}, 'replaceNN turns a NaN budget into 0, which fires')

    def test_no_data_is_ok(self) -> None:
        self.assertEqual(rules()['litellm-budget-low']['noDataState'], 'OK')


class GluetunScrapeTests(unittest.TestCase):
    def test_no_static_target_points_at_gluetun(self) -> None:
        for name, job in jobs().items():
            for group in job.get('static_configs', []):
                for target in group['targets']:
                    self.assertNotIn('gluetun', target, f'job {name} scrapes gluetun without the vpn profile')

    def test_gluetun_job_is_discovered_by_dns(self) -> None:
        job = jobs()['gluetun']
        self.assertEqual(job['dns_sd_configs'], [{'names': ['gluetun'], 'type': 'A', 'port': 8001}])

    def test_gluetun_job_scrapes_by_name(self) -> None:
        job = jobs()['gluetun']
        self.assertEqual(relabelled(job, '__address__'), [GLUETUN_METRICS])
        self.assertEqual(relabelled(job, 'instance'), [GLUETUN_METRICS])


class SearxngProbeTests(unittest.TestCase):
    def test_probe_is_discovered_by_dns(self) -> None:
        job = jobs()['searxng']
        self.assertEqual(job['dns_sd_configs'], [{'names': ['gluetun'], 'type': 'A', 'port': 8080}])
        self.assertNotIn('static_configs', job)

    def test_probe_targets_searxng_through_blackbox(self) -> None:
        job = jobs()['searxng']
        blackbox = jobs()['blackbox']
        self.assertEqual(relabelled(job, 'probe'), ['searxng'])
        self.assertEqual(relabelled(job, '__param_target'), [SEARXNG_TARGET])
        self.assertEqual(relabelled(job, 'instance'), [SEARXNG_TARGET])
        self.assertEqual(relabelled(job, '__address__'), relabelled(blackbox, '__address__'))
        self.assertEqual(job['metrics_path'], blackbox['metrics_path'])
        self.assertEqual(job['params'], blackbox['params'])

    def test_blackbox_keeps_only_host_probes(self) -> None:
        probes = [group['labels']['probe'] for group in jobs()['blackbox']['static_configs']]
        self.assertEqual(probes, ['ollama', 'comfyui'])


@unittest.skipUnless(docker_available(), 'docker is not available')
class PromtoolTests(unittest.TestCase):
    def promtool(self, folder: Path, *arguments: str) -> None:
        result = subprocess.run(
            ['docker', 'run', '--rm', '--network', 'none', '--entrypoint', 'promtool',
             '--volume', f'{folder}:/check:ro', prometheus_image(), *arguments],
            capture_output=True, text=True, timeout=300,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_budget_query_ignores_unlimited_keys(self) -> None:
        [query] = prometheus_queries(rules()['litellm-budget-low'])
        budget = 'litellm_remaining_api_key_budget_metric'
        unlimited = {'series': f'{budget}{{api_key_alias="unlimited"}}', 'values': 'Inf Inf Inf'}
        limited = {'series': f'{budget}{{api_key_alias="limited"}}', 'values': '0.5 0.5 0.5'}
        tests = {
            'rule_files': ['rules.yml'],
            'tests': [
                {
                    'interval': '1m',
                    'input_series': [unlimited],
                    'promql_expr_test': [{'expr': BUDGET_RECORD, 'eval_time': '2m', 'exp_samples': []}],
                },
                {
                    'interval': '1m',
                    'input_series': [unlimited, limited],
                    'promql_expr_test': [{
                        'expr': BUDGET_RECORD,
                        'eval_time': '2m',
                        'exp_samples': [{'labels': BUDGET_RECORD, 'value': 0.5}],
                    }],
                },
            ],
        }
        document = {'groups': [{'name': 'budget', 'interval': '1m', 'rules': [{'record': BUDGET_RECORD, 'expr': query}]}]}
        with TemporaryDirectory() as folder:
            (Path(folder) / 'rules.yml').write_text(yaml.safe_dump(document))
            (Path(folder) / 'tests.yml').write_text(yaml.safe_dump(tests))
            self.promtool(Path(folder), 'test', 'rules', '/check/tests.yml')

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
