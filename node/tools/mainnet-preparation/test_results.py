"""The results collector: fault records reduced to a public campaign summary,
judged against the approved targets, signed with the results key and read by
the launch page's generator. The release CLI is a stand-in here; the CI
fault drill runs the real one."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
from types import SimpleNamespace
import unittest

import faults
import launch_dashboard
import results

NATIVE, ENGINE = b'{"accounts": []}', b'{"genesis_time": "2026-11-02T13:49:45Z"}'
TIMINGS = {'fault_recovery_target_seconds': 120, 'validator_restart_target_seconds': 300}
RELEASE = {'schema': 'dytallix.release-build-record.v1', 'release_set_sha256': 'ab' * 32}
COMMIT = 'cd' * 20
PLAN = 'launch/approvals/P01_T06_SIMULATION_PLAN_2026-10-11.json'


def row(name, seconds, at='2026-11-03T10:00:00+00:00'):
    """A recovered scenario whose recovery took `seconds`."""
    cleared = faults.instant(at).timestamp() - seconds
    return {'name': name, 'command': {'command': 'fault-x', 'args': {}, 'status': 'DONE'}, 'clear': {},
            'effect': {'installed': True} if name == 'partition' else {'active': 'failed'},
            'baseline_height': 100, 'height_at_clear': 104, 'recovered_height': 110,
            'injected_at': faults.iso_at(cleared - 30), 'cleared_at': faults.iso_at(cleared), 'recovered_at': at}


def record(rows, target=120, host='sentry-1'):
    return json.dumps(faults.build_run('dytallix-staging-2', NATIVE, ENGINE, rows, target, host)).encode()


def summarize(*files, timings=TIMINGS):
    return results.summarize_campaign(list(files), NATIVE, ENGINE, timings, RELEASE, COMMIT)


def kpi(summary, kid): return next(k for k in summary['kpis'] if k['id'] == kid)


class Summarize(unittest.TestCase):
    def test_partitions_and_validator_restarts_become_campaign_kpis(self):
        sentry = record([row('latency', 9), row('partition', 21.04), row('partition', 30.0, '2026-11-04T09:00:00+00:00'),
                         row('process_kill', 12)])
        validator = record([row('validator_restart', 41.0)], target=300, host='validator-1')
        summary = summarize(sentry, validator)
        self.assertEqual(kpi(summary, 'campaign.faults.network_heal'),
                         {'id': 'campaign.faults.network_heal', 'status': 'in_progress', 'value': 30.0, 'unit': 's',
                          'samples': 2, 'target': 120, 'repetitions': 30})
        self.assertEqual(kpi(summary, 'campaign.faults.validator_restart')['value'], 41.0)
        self.assertEqual(summary['date'], '2026-11-04', 'the newest recovery\'s day')
        self.assertEqual(summary['candidate'], {'commit': COMMIT, 'release_set_sha256': 'ab' * 32})
        self.assertEqual([s['sha256'] for s in summary['sources']],
                         sorted(hashlib.sha256(raw).hexdigest() for raw in (sentry, validator)))
        self.assertEqual(results.summary_problems(summary), [])

    def test_one_slow_recovery_fails_and_the_repetitions_pass(self):
        slow = summarize(record([row('partition', 20), row('partition', 121)]))
        self.assertEqual((kpi(slow, 'campaign.faults.network_heal')['status'],
                          kpi(slow, 'campaign.faults.network_heal')['value']), ('fail', 121.0))
        enough = summarize(record([row('partition', 20 + i) for i in range(30)]))
        self.assertEqual(kpi(enough, 'campaign.faults.network_heal')['status'], 'pass')

    def test_the_approved_target_is_required(self):
        with self.assertRaisesRegex(results.Failed, 'judged against 600 s, not the approved 120 s'):
            summarize(record([row('partition', 20)], target=600))
        with self.assertRaisesRegex(results.Failed, 'not the approved 300 s'):
            summarize(record([row('validator_restart', 20)], target=120))

    def test_an_edited_record_or_another_genesis_is_refused(self):
        raw = json.loads(record([row('partition', 20)]))
        raw['scenarios'][0]['recovery_seconds'] = 5
        with self.assertRaisesRegex(results.Failed, 'does not verify'):
            summarize(json.dumps(raw).encode())
        with self.assertRaisesRegex(results.Failed, 'does not verify'):
            results.summarize_campaign([record([row('partition', 20)])], b'{}', ENGINE, TIMINGS, RELEASE, COMMIT)

    def test_nothing_but_numbers_leaves_the_evidence(self):
        # The fault records hold addresses (the peer drop's listings); none is copied.
        drop = row('peer_drop', 3)
        drop['effect'] = {'closed': ['10.77.0.11:26656 10.77.0.1:41234'], 'after': []}
        summary = summarize(record([drop, row('partition', 20)]))
        self.assertNotIn('10.77.0', results.render(summary))
        self.assertEqual(results.summary_problems(summary), [])
        leaky = copy.deepcopy(summary)
        leaky['kpis'][0]['note'] = 'sentry at 10.77.0.11'
        self.assertIn('contains an address', ' '.join(results.summary_problems(leaky)))

    def test_a_summary_needs_a_result_kpi_and_a_fault_recovery(self):
        with self.assertRaisesRegex(results.Failed, 'no recovery for a campaign KPI'):
            summarize(record([row('latency', 9)]))
        summary = summarize(record([row('partition', 20)]))
        summary['kpis'][0]['id'] = 'plan.d14_q02.approval'
        self.assertIn('is not a KPI the page reads from results', ' '.join(results.summary_problems(summary)))


class FakeCli:
    """`dytallix crypto sign` and `verify`: a signature is a digest of the text and key."""

    def __init__(self, key): self.key = key

    @staticmethod
    def signature(text, key): return (hashlib.sha512((text + key).encode()).hexdigest() * 52)[:6618]

    def __call__(self, args, env=None, capture_output=True, text=True):
        if args[1:3] == ['crypto', 'sign']:
            return SimpleNamespace(returncode=0, stdout=f'signature:\n{self.signature(args[3], self.key)}\n', stderr='')
        ok = args[1:3] == ['crypto', 'verify'] and args[4] == self.signature(args[3], args[5])
        return SimpleNamespace(returncode=0 if ok else 1, stdout='', stderr='')


class SignAndRead(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        for path in (launch_dashboard.DEFINITIONS, launch_dashboard.WORKFLOW, PLAN):
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(launch_dashboard.ROOT / path, self.root / path)
        self.key = '5a' * (results.PUBLIC_KEY_HEX // 2)
        key_id = hashlib.sha256(bytes.fromhex(self.key)).hexdigest()
        (self.root / results.KEYS).write_text(json.dumps({'schema': results.KEY_SCHEMA, 'keys': [
            {'key_id': key_id, 'since': '2026-11-02', 'public_key_hex': self.key}]}))
        self.home = self.root / 'home'
        (self.home / '.dytallix').mkdir(parents=True)
        (self.home / '.dytallix' / 'keystore.json').write_text(json.dumps({'active': 'launch-results', 'entries': [
            {'name': 'launch-results', 'public_key': list(bytes.fromhex(self.key))}]}))
        self.cli = FakeCli(self.key)
        summary = summarize(record([row('partition', 20), row('partition', 31.5, '2026-11-04T09:00:00+00:00')]))
        self.path = results.write_summary(summary, self.root)

    def test_sign_then_verify_and_an_edit_breaks_it(self):
        results.sign('dytallix', self.home, self.path, self.root, run=self.cli)
        self.assertEqual(len(Path(f'{self.path}.sig').read_text().strip()), 6618)
        key_id = results.verify('dytallix', self.path, self.root, run=self.cli)
        self.assertEqual(key_id, json.loads((self.root / results.KEYS).read_text())['keys'][0]['key_id'])
        self.path.write_bytes(self.path.read_bytes().replace(b'31.5', b'11.5'))
        with self.assertRaisesRegex(results.Failed, 'verifies under no key'):
            results.verify('dytallix', self.path, self.root, run=self.cli)

    def test_only_the_listed_results_key_signs(self):
        (self.home / '.dytallix' / 'keystore.json').write_text(json.dumps({'active': 'staging-command', 'entries': []}))
        with self.assertRaisesRegex(results.Failed, 'active wallet is not launch-results'):
            results.sign('dytallix', self.home, self.path, self.root, run=self.cli)

    def test_the_page_shows_the_newest_signed_summary(self):
        with self.assertRaisesRegex(launch_dashboard.Invalid, 'has no signature'):
            launch_dashboard.gather(self.root)
        results.sign('dytallix', self.home, self.path, self.root, run=self.cli)
        data = launch_dashboard.build(launch_dashboard.gather(self.root), 0)
        heal = next(k for k in data['kpis'] if k['id'] == 'campaign.faults.network_heal')
        self.assertEqual((heal['status'], heal['value'], heal['updated'], heal['target']),
                         ('in_progress', 31.5, '2026-11-04', 120))
        self.assertEqual(heal['evidence_url'],
                         f'{launch_dashboard.REPO}/blob/main/launch/simulations/results/campaign/2026-11-04.json')
        self.assertEqual(data['as_of'], '2026-11-04')
        self.assertEqual(launch_dashboard.page_problems(data), [])
        self.assertIn('launch/simulations/results/campaign/2026-11-04.json.sig',
                      [i['path'] for i in data['source']['inputs']])
        # The campaign phase is under way once one of its KPIs is.
        self.assertEqual(next(p for p in data['phases'] if p['id'] == 'campaign')['status'], 'in_progress')

    def test_the_page_refuses_a_summary_it_cannot_show(self):
        results.sign('dytallix', self.home, self.path, self.root, run=self.cli)
        good = self.path.read_text()
        summary = json.loads(good)
        summary['kpis'][0]['id'] = 'runs.r01.load'  # does not apply to R01
        self.path.write_text(results.render(summary))
        with self.assertRaisesRegex(launch_dashboard.Invalid, 'runs.r01.load is not a KPI the page reads from results'):
            launch_dashboard.gather(self.root)
        self.path.write_text(good)
        (self.root / launch_dashboard.RESULTS / 'campaign' / 'notes.txt').write_text('x')
        with self.assertRaisesRegex(launch_dashboard.Invalid, 'not a result summary or its signature'):
            launch_dashboard.gather(self.root)


if __name__ == '__main__':
    unittest.main()
