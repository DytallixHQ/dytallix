"""The approved network identity and genesis time procedure (D13-Q01)."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import network_identity as n

HERE = Path(__file__).resolve().parent


class NetworkIdentityTests(unittest.TestCase):
    def setUp(self): self.identity = n.load()

    def test_the_approved_identity(self):
        self.assertEqual((self.identity['chain_id'], self.identity['network']), ('dytallix-mainnet-1', 'mainnet'))
        self.assertEqual(self.identity['display_names']['mainnet'], 'Dytallix')
        self.assertEqual(self.identity['address'], {'network_code': 1, 'prefix': 'dytallix'})
        approval = json.loads((n.IDENTITY.parents[1]/self.identity['approval_record']).read_text())
        self.assertEqual(approval['decisions'][0]['selected'], self.identity['chain_id'])
        # The genesis builder takes at most 50 bytes from [A-Za-z0-9._-]; a
        # development build refuses a chain naming mainnet.
        chain = self.identity['chain_id']
        self.assertTrue(len(chain) <= 50 and all(c.isalnum() or c in '._-' for c in chain) and 'mainnet' in chain)

    def test_the_genesis_time_is_set_at_the_freeze(self):
        changed = copy.deepcopy(self.identity)
        changed['genesis_time']['value'] = '2027-01-07T14:00:00Z'
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'IDENTITY.json'
            path.write_text(json.dumps(changed))
            with self.assertRaises(ValueError): n.load(path)

    def test_earliest_is_a_weekday_at_14_utc_at_least_72_hours_later(self):
        for built, expected in (
            ('2027-01-04T09:30:00Z', '2027-01-07T14:00:00Z'),  # Monday morning: Thursday
            ('2027-01-04T14:00:00Z', '2027-01-07T14:00:00Z'),  # exactly 72 hours
            ('2027-01-04T14:00:01Z', '2027-01-08T14:00:00Z'),  # one second late: Friday
            ('2027-01-05T15:00:00Z', '2027-01-11T14:00:00Z'),  # Saturday and Sunday skipped
        ):
            self.assertEqual(n.earliest(self.identity, built), expected, built)
            self.assertEqual(n.time_errors(self.identity, expected, built), [])

    def test_a_time_breaking_the_procedure_is_refused(self):
        built = '2027-01-04T09:30:00Z'
        for genesis_time, reason in (
            ('2027-01-07T14:00:01Z', '14:00:00'),
            ('2027-01-09T14:00:00Z', 'Friday'),
            ('2027-01-06T14:00:00Z', '72 hours'),
            ('2027-01-07 14:00:00', 'whole seconds'),
            (None, 'whole seconds'),
        ):
            errors = n.time_errors(self.identity, genesis_time, built)
            self.assertTrue(any(reason in e for e in errors), (genesis_time, errors))

    def test_records_must_name_the_identity(self):
        records = {'chain_id': 'dytallix-mainnet-1', 'network': 'mainnet', 'genesis_time': '2027-01-07T14:00:00Z'}
        self.assertEqual(n.record_errors(self.identity, records), [])
        for field, value in (('chain_id', 'dytallix-staging-1'), ('network', 'testnet')):
            changed = dict(records, **{field: value})
            self.assertTrue(any(field in e for e in n.record_errors(self.identity, changed)))

    def test_the_cli(self):
        command = [sys.executable, '-B', str(HERE/'network_identity.py')]
        done = subprocess.run(command + ['earliest', '--built-at', '2027-01-04T09:30:00Z'], capture_output=True, text=True)
        self.assertEqual((done.returncode, json.loads(done.stdout)['genesis_time']), (0, '2027-01-07T14:00:00Z'), done.stderr)
        good = subprocess.run(command + ['check', '--genesis-time', '2027-01-07T14:00:00Z', '--built-at', '2027-01-04T09:30:00Z'], capture_output=True)
        self.assertEqual(good.returncode, 0)
        late = subprocess.run(command + ['check', '--genesis-time', '2027-01-06T14:00:00Z', '--built-at', '2027-01-04T09:30:00Z'], capture_output=True, text=True)
        self.assertEqual(late.returncode, 2)
        self.assertEqual(json.loads(late.stdout)['status'], 'BREAKS_PROCEDURE')


if __name__ == '__main__': unittest.main(verbosity=2)
