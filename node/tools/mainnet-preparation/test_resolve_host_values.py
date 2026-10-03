"""The host values resolver (E05). No production value is selected."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import resolve_host_values as r

HERE = Path(__file__).resolve().parent
LAUNCH = HERE.parents[2]/'launch'
REHEARSAL = HERE/'fixtures'/'host-config-rehearsal'
# The values dytallix-host-config writes into each host's engine files.
HOST_PATHS = ('config.toml', 'config/pqc_transport.json')


class HostValuesTests(unittest.TestCase):
    def setUp(self):
        self.values = json.loads((LAUNCH/'E05_VALUES.json').read_text())
        self.proposals = json.loads((LAUNCH/'hosts'/'PROPOSALS.json').read_text())

    def resolve(self): return r.resolve(self.values, self.proposals)

    def refused(self, message):
        with self.assertRaises(ValueError) as caught: self.resolve()
        self.assertIn(message, str(caught.exception))

    def entry(self, name): return next(v for v in self.values['values'] if v['name'] == name)

    def test_the_committed_rehearsal_is_current(self):
        host, report = self.resolve()
        self.assertEqual(r.render(host), (REHEARSAL/'host-values.json').read_text(), 'rerun resolve_host_values.py')
        self.assertEqual(r.render(report), (REHEARSAL/'host-values-resolution.json').read_text(), 'rerun resolve_host_values.py')
        # Labeled placeholders keep it ineligible.
        self.assertFalse(report['production_eligible'])
        self.assertEqual(report['counts'], {'APPROVED': 30, 'MEASURE_PLACEHOLDER': 4})

    def test_the_cli_checks_the_committed_files(self):
        command = [sys.executable, '-B', str(HERE/'resolve_host_values.py'), '--values', str(LAUNCH/'E05_VALUES.json'),
                   '--proposals', str(LAUNCH/'hosts'/'PROPOSALS.json')]
        done = subprocess.run(command + ['--host-values', str(REHEARSAL/'host-values.json'),
                                         '--resolution', str(REHEARSAL/'host-values-resolution.json'), '--check'], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        with tempfile.TemporaryDirectory() as tmp:
            stale = subprocess.run(command + ['--host-values', str(Path(tmp)/'v.json'), '--resolution', str(Path(tmp)/'r.json'), '--check'],
                                   capture_output=True, text=True)
            self.assertEqual(stale.returncode, 1)

    def test_every_host_value_is_resolved(self):
        for v in self.values['values']:
            if v['path'].startswith(HOST_PATHS):
                self.assertIn(v['name'], r.VALUES, f'{v["name"]} ({v["path"]}) has no place in the host values')
        for name in r.VALUES:
            self.assertTrue(self.entry(name)['path'].startswith(HOST_PATHS), name)

    def test_eligibility_needs_every_value_approved(self):
        for name, proposal in self.proposals['values'].items():
            self.entry(name).update(status='APPROVED', approved=proposal['value'], approval_record='approvals/test.json')
        self.proposals['values'] = {}
        self.assertTrue(self.resolve()[1]['production_eligible'])

    def test_approved_values_are_used_as_approved(self):
        host, report = self.resolve()
        self.assertEqual(host['consensus']['double_sign_check_height'], {'validator': 10, 'sentry': 0, 'endpoint': 0})
        self.assertEqual(host['p2p']['persistent_peers_max_dial_period'], '60s')
        self.assertEqual(host['transport']['handshake_timeout_ms'], 5000)
        source = {v['name']: v['source'] for v in report['values']}
        self.assertEqual(source['timeout_commit'], 'APPROVED')
        self.assertEqual(source['mempool_size'], 'MEASURE_PLACEHOLDER')

    def test_a_proposal_for_an_approved_value_is_refused(self):
        self.proposals['values']['timeout_commit'] = {'value': '5s', 'status': 'PROPOSED', 'basis': 'stale'}
        self.refused('proposals for approved values')

    def test_an_open_value_needs_a_proposal(self):
        del self.proposals['values']['p2p_send_rate']
        self.refused('p2p_send_rate: open with no proposal')

    def test_unknown_and_unused_names_are_refused(self):
        self.proposals['values']['no_such_value'] = {'value': 1, 'status': 'PROPOSED', 'basis': 'x'}
        self.refused('unknown values')
        del self.proposals['values']['no_such_value']
        self.proposals['values']['queue_max_entries'] = {'value': 1, 'status': 'PROPOSED', 'basis': 'x'}
        self.refused('does not use')

    def test_a_proposal_carries_a_label(self):
        self.proposals['values']['mempool_size']['status'] = 'APPROVED'
        self.refused('proposal status')
        self.proposals['values']['mempool_size'] = {'value': 5000, 'status': 'PROPOSED'}
        self.refused('exact fields')

    def test_values_keep_their_types(self):
        for name, value in (('mempool_size', '5000'), ('mempool_size', -1), ('mempool_size', True), ('p2p_send_rate', 1.5)):
            self.setUp(); self.proposals['values'][name]['value'] = value
            self.refused(name)
        for name, value in (('timeout_commit', '4 s'), ('timeout_commit', '1m30s'), ('timeout_commit', 4),
                            ('skip_timeout_commit', 0), ('double_sign_check_height', {'validator': 10, 'sentry': 0})):
            self.setUp(); self.entry(name)['approved'] = value
            self.refused(name)


if __name__ == '__main__': unittest.main(verbosity=2)
