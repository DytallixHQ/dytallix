"""The upgrade drill's evidence (simulation plan, T8): each rule of the
handover with its migration, what makes it PASS, FAIL or INCONCLUSIVE, and
its digest-bound record. The real run is the Host install workflow's
upgrade drill."""
import copy
import json
import unittest

import upgrade_drill as ud

GENESIS = b'{"accounts": []}'
RC1, RC2 = '11' * 64, '22' * 64


def observations():
    """A handover that followed the procedure: admitted at 100, 20-block
    notice, activated after 120, both on RC2 at 130."""
    node = {'height': 130, 'block_hash': 'b130', 'app_hash': 'a129'}
    return {
        'releases': {'source': RC1, 'target': RC2}, 'notice_blocks': 20,
        'reconcile_before': {'status': 'PASS', 'height': 90, 'sha256': 'r1'},
        'admitted': {'height': 101, 'upgrade_pending': {'admitted_height': 99, 'plan': {}},
                     'handover_pending': {'plan': {'target_release_sha512': RC2}, 'admitted_height': 100}},
        'activation_anchor_height': 121, 'halt_height': 123,
        'stopped': {'validator-1': {'active': 'failed', 'release_refusal': True},
                    'sentry-1': {'active': 'inactive', 'release_refusal': True}},
        'units_after': {'validator-1': RC2[:16], 'sentry-1': RC2[:16]},
        'active': {'release': RC2, 'handover_schema': 1, 'upgrade_schema': 1},
        'compared': {'validator': dict(node), 'sentry': dict(node)},
        'host_verify_exit': {'validator-1': 0, 'sentry-1': 0},
        'reconcile_after': {'status': 'PASS', 'height': 131, 'sha256': 'r2'},
    }


def checks(obs):
    rows, status = ud.evaluate(obs)
    return {r['check']: r['status'] for r in rows}, status


class UpgradeDrillTests(unittest.TestCase):
    def test_a_handover_by_the_procedure_passes(self):
        found, status = checks(observations())
        self.assertEqual(status, ud.PASS, found)
        self.assertEqual(set(found), {'admitted', 'notice', 'stopped', 'switched', 'active', 'resumed', 'host_verify',
                                      'reconciled'})

    def test_each_broken_rule_fails(self):
        cases = {
            'admitted': lambda o: o['admitted']['handover_pending']['plan'].update(target_release_sha512=RC1),
            'notice': lambda o: o.update(activation_anchor_height=118),
            'stopped': lambda o: o['stopped']['sentry-1'].update(active='active'),
            'switched': lambda o: o['units_after'].update({'sentry-1': RC1[:16]}),
            'active': lambda o: o['active'].update(handover_schema=0),
            'resumed': lambda o: o['compared']['sentry'].update(app_hash='other'),
            'host_verify': lambda o: o['host_verify_exit'].update({'validator-1': 1}),
            'reconciled': lambda o: o['reconcile_after'].update(status='FAIL'),
        }
        for check, change in cases.items():
            with self.subTest(check=check):
                obs = observations()
                change(obs)
                found, status = checks(obs)
                self.assertEqual((found[check], status), (ud.FAIL, ud.FAIL))
        # A stop without RC1's refusal of the committed release is not the activation's.
        obs = observations()
        obs['stopped']['validator-1']['release_refusal'] = False
        self.assertEqual(checks(obs)[0]['stopped'], ud.FAIL)
        # Resumed, but not past the activation.
        obs = observations()
        for node in obs['compared'].values():
            node['height'] = 124
        self.assertEqual(checks(obs)[0]['resumed'], ud.FAIL)
        # The same release twice is no handover.
        obs = observations()
        obs['releases']['target'] = RC1
        self.assertEqual(checks(obs)[0]['admitted'], ud.FAIL)

    def test_a_missing_reading_is_inconclusive(self):
        for key in ('admitted', 'activation_anchor_height', 'stopped', 'units_after', 'active', 'compared',
                    'host_verify_exit', 'reconcile_after'):
            with self.subTest(missing=key):
                obs = observations()
                del obs[key]
                found, status = checks(obs)
                self.assertEqual(status, ud.INCONCLUSIVE, found)

    def test_the_admission_height_is_found_wherever_the_view_nests_it(self):
        self.assertEqual(ud.admitted_height({'plan': {'x': 1}, 'receipt': {'admitted_height': 7}}), 7)
        self.assertIsNone(ud.admitted_height({'plan': {}}))
        self.assertIsNone(ud.admitted_height(None))

    def test_the_record_is_digest_bound(self):
        record = ud.build('dytallix-staging-1', GENESIS, observations())
        self.assertEqual(record['status'], ud.PASS)
        self.assertEqual(ud.verify(json.loads(json.dumps(record)), GENESIS), ud.PASS)
        edited = copy.deepcopy(record)
        edited['observations']['units_after']['sentry-1'] = RC1[:16]
        with self.assertRaisesRegex(ud.Failed, 'digest'):
            ud.verify(edited, GENESIS)
        with self.assertRaisesRegex(ud.Failed, 'another native genesis'):
            ud.verify(record, b'{}')
        # A record whose checks were written by hand, re-digested, is refused.
        forged = copy.deepcopy(record)
        forged['observations']['active']['handover_schema'] = 0
        body = {k: v for k, v in forged.items() if k != 'sha256'}
        forged['sha256'] = ud.digest(ud.canonical(body))
        with self.assertRaisesRegex(ud.Failed, 'do not follow'):
            ud.verify(forged, GENESIS)


if __name__ == '__main__':
    unittest.main()
