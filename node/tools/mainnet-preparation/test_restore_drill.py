"""The restore drill's evidence (disaster recovery v1, R6): each rule of the
restore, what makes it PASS, FAIL or INCONCLUSIVE, and its digest-bound
record. The real run is the Host install workflow's restore drill."""
import copy
import json
import unittest

import restore_drill as rd

GENESIS = b'{"accounts": []}'
TARGET = 4 * 3600


def observations():
    """A restore by the runbook: the copy of height 20, caught up at 60."""
    instants = {step: 1_800_000_000 + 60 * i for i, step in enumerate(rd.STEPS)}
    node = {'height': 60, 'block_hash': 'b60', 'app_hash': 'a59'}
    return {'clock': 'the runner', 'instants': instants,
            'copy': {'height': 20, 'sha256': 'c' * 64, 'parts': 1},
            'compared': {'validator': dict(node), 'sentry': dict(node)}, 'host_verify_exit': 0}


def checks(obs, target=TARGET):
    rows, status = rd.evaluate(obs, target)
    return {r['check']: r['status'] for r in rows}, status


class RestoreDrillTests(unittest.TestCase):
    def test_a_restore_by_the_runbook_passes(self):
        found, status = checks(observations())
        self.assertEqual(status, rd.PASS, found)
        self.assertEqual(set(found), {'order', 'copy', 'state', 'host_verify', 'objective'})

    def test_each_broken_rule_fails(self):
        cases = {
            'order': lambda o: o['instants'].update(installed_at=o['instants']['declared_at'] - 1),
            'copy': lambda o: [n.update(height=19) for n in o['compared'].values()],
            'state': lambda o: o['compared']['sentry'].update(app_hash='other'),
            'host_verify': lambda o: o.update(host_verify_exit=1),
            'objective': lambda o: o['instants'].update(caught_up_at=o['instants']['declared_at'] + TARGET + 1),
        }
        for check, change in cases.items():
            with self.subTest(check=check):
                obs = observations()
                change(obs)
                found, status = checks(obs)
                self.assertEqual((found[check], status), (rd.FAIL, rd.FAIL))

    def test_a_missing_reading_is_inconclusive(self):
        for key in ('instants', 'copy', 'compared', 'host_verify_exit'):
            with self.subTest(missing=key):
                obs = observations()
                del obs[key]
                self.assertEqual(checks(obs)[1], rd.INCONCLUSIVE)

    def test_the_approved_objective_is_four_hours(self):
        self.assertEqual(rd.approved_target(), TARGET)

    def test_the_record_is_digest_bound(self):
        record = rd.build('dytallix-staging-1', GENESIS, observations(), TARGET)
        self.assertEqual(rd.verify(json.loads(json.dumps(record)), GENESIS), rd.PASS)
        edited = copy.deepcopy(record)
        edited['observations']['host_verify_exit'] = 1
        with self.assertRaisesRegex(rd.Failed, 'digest'):
            rd.verify(edited, GENESIS)
        with self.assertRaisesRegex(rd.Failed, 'another native genesis'):
            rd.verify(record, b'{}')
        loose = rd.build('dytallix-staging-1', GENESIS, observations(), TARGET * 2)
        with self.assertRaisesRegex(rd.Failed, 'not the approved'):
            rd.verify(loose, GENESIS)


if __name__ == '__main__':
    unittest.main()
