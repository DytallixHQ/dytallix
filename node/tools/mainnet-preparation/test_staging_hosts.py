"""The staging hosts record and its summary, staging.hosts.ready. Every
input here is synthetic: no real host's verify, country or bundle."""
import copy
import json
from pathlib import Path
import shutil
import tempfile
import unittest

import launch_dashboard
import recovery
import results
import staging_hosts as sh
import test_recovery
import test_results

NATIVE = test_results.NATIVE
CHAIN = 'dytallix-staging-2'
STAGING = {'status': 'STAGING_CHAIN_BUILT', 'chain_id': CHAIN, 'bundles': {
    'validator-1': {'path': 'validator-1.bundle.tar', 'sha256': '11' * 32, 'role': 'validator'},
    'sentry-1': {'path': 'sentry-1.bundle.tar', 'sha256': '22' * 32, 'role': 'sentry'}}}


def t9(exit_code=0):
    obs = test_recovery.observations()
    obs['host_verify_exit'] = exit_code
    return json.dumps(recovery.build(CHAIN, NATIVE, obs, recovery.approved_target())).encode()


def observations(**changes):
    obs = {'verified_at': '2026-11-14T10:00:00Z', 'countries': {'validator': 'DE', 'sentry': 'FI'},
           'sentry_verify_exit': 0, 'sentry_bundle_tar_sha256': None,
           'public': {'sentry_status_page': True, 'sentry_client_channel': True, 'validator_no_public_path': True}}
    obs.update(changes)
    return obs


def hosts(obs=None, staging=STAGING, recovery_raw=None):
    return sh.build(staging, NATIVE, recovery_raw or t9(), obs or observations())


def summarize(record, recovery_raw=None, staging=STAGING):
    return results.summarize_staging_hosts(json.dumps(record).encode(), staging, NATIVE, recovery_raw or t9())


class Record(unittest.TestCase):
    def test_both_hosts_verified_in_different_countries_pass(self):
        record = hosts()
        self.assertEqual(record['status'], sh.PASS)
        self.assertEqual(sh.verify(record, STAGING, NATIVE, t9()), sh.PASS)
        self.assertEqual(sh.verified(record), 2)
        self.assertEqual(record['recovery']['host_verify_exit'], 0)

    def test_a_failed_verify_same_country_or_a_public_validator_fail(self):
        for obs, check in ((observations(sentry_verify_exit=1), 'sentry_verify'),
                           (observations(countries={'validator': 'DE', 'sentry': 'DE'}), 'countries'),
                           (observations(sentry_bundle_tar_sha256='33' * 32), 'sentry_bundle'),
                           (observations(public={'sentry_status_page': True, 'sentry_client_channel': True,
                                                 'validator_no_public_path': False}), 'public')):
            record = hosts(obs)
            self.assertEqual(record['status'], sh.FAIL, check)
            self.assertEqual(next(r for r in record['checks'] if r['check'] == check)['status'], sh.FAIL)
        failed = hosts(recovery_raw=t9(2))
        self.assertEqual((failed['status'], sh.verified(failed)), (sh.FAIL, 1))
        self.assertEqual(hosts(observations(sentry_bundle_tar_sha256='22' * 32))['status'], sh.PASS)

    def test_a_missing_reading_is_inconclusive(self):
        for obs in (observations(sentry_verify_exit=None), observations(countries={'validator': 'DE'}),
                    observations(public={'sentry_status_page': True})):
            self.assertEqual(hosts(obs)['status'], sh.INCONCLUSIVE)
        self.assertEqual(hosts(recovery_raw=t9(None))['status'], sh.INCONCLUSIVE)

    def test_the_chain_needs_one_validator_and_one_sentry_bundle(self):
        three = copy.deepcopy(STAGING)
        three['bundles']['sentry-2'] = {'path': 's2', 'sha256': '44' * 32, 'role': 'sentry'}
        self.assertEqual(next(r for r in hosts(staging=three)['checks'] if r['check'] == 'bundles')['status'], sh.FAIL)

    def test_an_edit_or_other_inputs_are_refused(self):
        record = hosts()
        edited = copy.deepcopy(record)
        edited['observations']['sentry_verify_exit'] = 1
        with self.assertRaisesRegex(sh.Failed, 'digest does not match'):
            sh.verify(edited, STAGING, NATIVE, t9())
        other = copy.deepcopy(STAGING)
        other['bundles']['sentry-1']['sha256'] = '55' * 32
        with self.assertRaisesRegex(sh.Failed, 'other bundles'):
            sh.verify(record, other, NATIVE, t9())
        with self.assertRaisesRegex(sh.Failed, 'another T9 recovery record'):
            sh.verify(record, STAGING, NATIVE, t9(1))
        with self.assertRaisesRegex(sh.Failed, 'T9 recovery record does not verify'):
            sh.verify(record, STAGING, b'{"other": 1}', t9())


class Summary(unittest.TestCase):
    def test_the_summary_is_the_count_and_passes_the_page_checks(self):
        summary = summarize(hosts())
        self.assertEqual(summary['scope'], 'staging')
        self.assertEqual(summary['date'], '2026-11-14')
        self.assertEqual(summary['kpis'], [{'id': 'staging.hosts.ready', 'status': 'pass', 'value': 2,
                                            'unit': 'count', 'target': 2}])
        self.assertEqual(sorted(s['tool'] for s in summary['sources']), ['recovery', 'staging_hosts'])
        self.assertEqual(results.summary_problems(summary), [])
        text = json.dumps(summary)
        for private in ('"DE"', '"FI"', 'sentry-1', 'validator-1', '11' * 32):
            self.assertNotIn(private, text)

    def test_statuses_follow_the_record(self):
        self.assertEqual(summarize(hosts(observations(sentry_verify_exit=3)))['kpis'][0]['status'], 'fail')
        pending = summarize(hosts(observations(public={})))['kpis'][0]
        self.assertEqual((pending['status'], pending['value']), ('in_progress', 2))
        with self.assertRaisesRegex(results.Failed, 'staging hosts record does not verify'):
            summarize(hosts(), recovery_raw=t9(1))

    def test_the_page_reads_the_signed_staging_summary(self):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        for path in (launch_dashboard.DEFINITIONS, launch_dashboard.WORKFLOW, test_results.PLAN):
            (root / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(launch_dashboard.ROOT / path, root / path)
        path = results.write_summary(summarize(hosts()), root)
        self.assertEqual(path, root / launch_dashboard.RESULTS / 'staging' / '2026-11-14.json')
        path.with_name(path.name + '.sig').write_text('ab' * 3309 + '\n')
        data = launch_dashboard.build(launch_dashboard.gather(root), 0)
        ready = next(k for k in data['kpis'] if k['id'] == 'staging.hosts.ready')
        self.assertEqual((ready['status'], ready['value'], ready['updated']), ('pass', 2, '2026-11-14'))
        self.assertEqual(launch_dashboard.page_problems(data), [])


if __name__ == '__main__':
    unittest.main()
