"""The launch page's data follows the repository's records and passes the page's checks."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import launch_dashboard as m

HERE = Path(__file__).resolve().parent
PLAN = 'launch/approvals/P01_T06_SIMULATION_PLAN_2026-10-11.json'


class LaunchDashboardTests(unittest.TestCase):
    def setUp(self):
        self.found = m.gather()
        self.data = m.build(self.found, 0)

    def changed(self, edit):
        """The dashboard built from definitions that edit() changed."""
        found = copy.deepcopy(self.found)
        edit(found['definitions'])
        return m.build(found, 0)

    def kpi(self, data, kid): return next(k for k in data['kpis'] if k['id'] == kid)

    def test_the_committed_dashboard_is_current(self):
        self.assertEqual(m.stale(), [], 'run tools/mainnet-preparation/launch_dashboard.py --write')

    def test_it_passes_the_page_checks_and_this_repositorys(self):
        self.assertEqual(m.page_problems(self.data), [])
        self.assertEqual(m.problems(self.data), [])
        self.assertGreaterEqual(self.data['as_of'], m.SITE_COPY_AS_OF, 'the page keeps its own copy over an older one')

    def test_the_page_refuses_addresses_but_not_times_or_code_paths(self):
        for value in ('the host at 203.0.113.7', 'peer 2001:db8::3', '2001:db8:0:0:0:0:0:3'):
            self.assertTrue(m.contains_address(value), value)
        for value in ('genesis at 14:00:00 UTC', 'fee::adapt', 'T+15m', 'R01 to R06'):
            self.assertFalse(m.contains_address(value), value)
        data = copy.deepcopy(self.data)
        data['phases'][0]['summary'] = 'Runs on 203.0.113.7'
        self.assertEqual(m.page_problems(data), ['phases[0].summary contains an address'])

    def test_the_page_refuses_what_its_own_checks_refuse(self):
        data = copy.deepcopy(self.data)
        del data['kpis'][0]['value']
        data['kpis'][1]['unit'] = 'blocks'
        data['kpis'][2]['evidence_url'] = 'https://example.com/x'
        data['kpis'][3]['updated'] = '2026-11-02\n'
        data['kpis'][4]['target'] = '3000'
        self.assertEqual(m.page_problems(data), ['kpis[0].value must be text', 'kpis[1].unit is invalid',
                                                 f'kpis[2].evidence_url must link to {m.REPO}',
                                                 'kpis[3].updated must be a date', 'kpis[4].target must be a number'])

    def test_launch_server_names_and_missing_evidence_are_refused(self):
        data = copy.deepcopy(self.data)
        data['groups'][0]['summary'] = 'The sentry on ubuntu-4gb-hel1-2'
        data['kpis'][0]['evidence_url'] = f'{m.REPO}/blob/main/launch/NO_SUCH_RECORD.json'
        found = m.problems(data)
        self.assertEqual(len(found), 2, found)
        self.assertIn('names a launch server (hel1, ubuntu-4gb)', found[0])
        self.assertIn('NO_SUCH_RECORD.json is not in this repository', found[1])

    def test_a_tool_passes_only_when_built_proven_and_complete(self):
        self.assertEqual(self.kpi(self.data, 'tools.t3.ready')['status'], 'pass')
        t8 = self.kpi(self.data, 'tools.t8.ready')
        self.assertEqual(t8['status'], 'in_progress')
        self.assertIn('still to come: a return handover', t8['value'])
        self.assertEqual(t8['updated'], '2026-11-02')
        self.assertEqual(self.kpi(self.data, 'plan.d14_q02.approval')['value'], 'Approved 11 October 2026')

    def test_a_tool_claim_must_match_its_ci_step(self):
        found = copy.deepcopy(self.found)
        found['workflow'] = found['workflow'].replace('(T8)', '(T8 later)')
        with self.assertRaisesRegex(m.Invalid, r'T8 is marked built, but no CI step "\(T8\)" runs'):
            m.build(found, 0)

        def unbuilt(d): next(k for k in d['kpis'] if k['id'] == 'tools.t8.ready')['source']['built'] = None
        with self.assertRaisesRegex(m.Invalid, 'T8 is not marked built, but its CI step runs'):
            self.changed(unbuilt)

    def test_result_kpis_wait_for_signed_summaries(self):
        self.assertTrue(all(k['value'] is None for k in self.data['kpis'] if 'results' in
                            next(d for d in self.found['definitions']['kpis'] if d['id'] == k['id'])['source']))
        self.assertEqual(self.kpi(self.data, 'runs.r01.load')['status'], 'not_applicable')
        self.assertEqual(self.kpi(self.data, 'campaign.transactions.attempts')['target'], 3000)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for path in (m.DEFINITIONS, m.WORKFLOW, PLAN):
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_bytes((m.ROOT / path).read_bytes())
            self.assertEqual(m.build(m.gather(root), 0)['kpis'], self.data['kpis'])
            # A summary counts only with its signature beside it (results.py signs).
            (root / m.RESULTS / 'run').mkdir(parents=True)
            (root / m.RESULTS / 'run' / '2026-11-03.json').write_text('{}')
            with self.assertRaisesRegex(m.Invalid, 'has no signature in hex beside it'):
                m.gather(root)

    def test_phases_roll_up_as_the_page_does(self):
        self.assertEqual({p['id']: p['status'] for p in self.data['phases']}['tools'], 'in_progress')
        self.assertEqual(m.roll_up(['pass', 'not_applicable']), 'pass')
        self.assertEqual(m.roll_up(['pass', 'fail', 'planned']), 'fail')
        self.assertEqual(m.roll_up(['planned', 'blocked', 'pass']), 'blocked')
        self.assertEqual(m.roll_up(['planned', 'not_applicable']), 'planned')
        self.assertEqual(m.roll_up(['not_applicable']), 'not_applicable')

        def unapproved(d): d['kpis'][0]['source'] = {'results': True}
        self.assertEqual({p['id']: p['status'] for p in self.changed(unapproved)['phases']}['plan'], 'planned')

    def test_the_check_ignores_only_the_commit_count(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for path in (m.DEFINITIONS, m.WORKFLOW, m.DASHBOARD, PLAN):
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_bytes((m.ROOT / path).read_bytes())
            data = json.loads((root / m.DASHBOARD).read_text())
            # The copy holds one approval record, so its own count is 1.
            data['project']['approval_records'] = 1
            data['project']['commits_at_as_of'] += 7
            (root / m.DASHBOARD).write_text(m.render(data))
            self.assertEqual([f for f in m.stale(root) if 'evidence' not in f], [])
            data['kpis'][0]['value'] = 'Approved by hand'
            (root / m.DASHBOARD).write_text(m.render(data))
            self.assertIn(f'{m.DASHBOARD} is stale', m.stale(root))

    def test_the_command_reports_a_pass(self):
        done = subprocess.run([sys.executable, '-B', str(HERE / 'launch_dashboard.py')], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(json.loads(done.stdout)['status'], 'PASS')


if __name__ == '__main__':
    unittest.main()
