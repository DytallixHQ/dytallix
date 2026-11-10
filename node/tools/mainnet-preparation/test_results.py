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
import unittest.mock
from unittest import mock

import attacks
import load
import endpoints
import faults
import launch_dashboard
import recovery
import restore_drill
import results
import test_endpoints
import test_recovery
import test_restore_drill

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


def summarize(*files, timings=TIMINGS, restores=(), recoveries=()):
    return results.summarize_campaign(list(files), NATIVE, ENGINE, timings, RELEASE, COMMIT, restores, recoveries)


def drill(tool, observations, seconds, target=None):
    """A heavy fault drill's record whose time, declared to its end, is `seconds`."""
    end = 'caught_up_at' if tool is restore_drill else 'first_block_after_halt_at'
    observations['instants'][end] = observations['instants']['declared_at'] + seconds
    target = tool.approved_target() if target is None else target
    return json.dumps(tool.build('dytallix-staging-2', NATIVE, observations, target)).encode()


def rebuild(seconds, **changes): return drill(restore_drill, test_restore_drill.observations(), seconds, **changes)


def host_loss(seconds): return drill(recovery, test_recovery.observations(), seconds)


def kpi(summary, kid): return next(k for k in summary['kpis'] if k['id'] == kid)


def attack_record(per_type, admitted=(), other=(), finished='2026-11-04T09:00:00Z'):
    """A transaction-family record: per_type refusals of each built type for
    its reason, the named types' attempts admitted or refused otherwise, and
    one committed control; with its rows."""
    rows = []
    for kind, reason in attacks.REASONS.items():
        if reason is None:
            continue
        code = -1 if kind in attacks.BEFORE_ADMISSION else 1
        # A forged type's rows carry the attack client's checks.
        forged = ({'forged': {'variant': kind, 'signature_verifies': True, 'profile_refusal': attacks.FORGED[kind]}}
                  if kind in attacks.FORGED else {})
        for k in range(per_type):
            r = {'type': kind, 'k': k, 'wallet': 'load-0001', 'code': code, 'log': reason, 'outcome': 'refused', **forged}
            if kind == 'resigned_copy':
                r['same_bytes'] = True
            rows.append(r)
        if kind in admitted:
            rows.append({'type': kind, 'k': per_type, 'wallet': 'load-0001', 'code': 0, 'log': '', 'outcome': 'admitted',
                         **forged})
        if kind in other:
            rows.append({'type': kind, 'k': per_type, 'wallet': 'load-0001', 'code': 4, 'log': 'another reason',
                         'outcome': 'refused_other', **forged})
    rows.append({'type': 'control', 'wallet': 'load-0001', 'code': 0, 'log': '', 'committed': True})
    record = attacks.build_record('dytallix-staging-2', NATIVE, ENGINE, rows, per_type, '2026-11-04T08:00:00Z', finished)
    return json.dumps(record).encode(), attacks.rows_bytes(rows)


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

    def test_a_fault_command_that_never_completed_fails_its_kpi(self):
        stuck = dict(row('validator_restart', 20), cleared_at=None, height_at_clear=None, recovered_at=None,
                     recovered_height=None, command={'command': 'fault-kill', 'args': {}, 'status': 'FAILED'})
        summary = summarize(record([stuck], target=300, host='validator-1'))
        self.assertEqual((kpi(summary, 'campaign.faults.validator_restart')['status'],
                          kpi(summary, 'campaign.faults.validator_restart')['value']), ('fail', None))
        self.assertEqual(summary['date'], stuck['injected_at'][:10], 'the injection\'s day')

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

    def test_heavy_faults_come_from_the_drills_own_evidence(self):
        summary = summarize(restores=[rebuild(3600), rebuild(5400.25)], recoveries=[host_loss(7200)])
        self.assertEqual(kpi(summary, 'campaign.faults.sentry_rebuild'),
                         {'id': 'campaign.faults.sentry_rebuild', 'status': 'in_progress', 'value': 5400.2,
                          'unit': 's', 'samples': 2, 'target': 14400, 'repetitions': 3})
        self.assertEqual(kpi(summary, 'campaign.faults.validator_host')['target'], 28800)
        self.assertEqual([s['tool'] for s in summary['sources']].count('restore'), 2)
        self.assertEqual(results.summary_problems(summary), [])
        # Three within the objective pass; one past it fails the KPI.
        self.assertEqual(kpi(summarize(restores=[rebuild(3600)] * 3), 'campaign.faults.sentry_rebuild')['status'],
                         'pass')
        late = summarize(restores=[rebuild(3600), rebuild(14401)])
        self.assertEqual((kpi(late, 'campaign.faults.sentry_rebuild')['status'],
                          kpi(late, 'campaign.faults.sentry_rebuild')['value']), ('fail', 14401.0))
        # With the quick faults, one cumulative summary.
        both = summarize(record([row('partition', 20)]), recoveries=[host_loss(600)])
        self.assertEqual([k['id'] for k in both['kpis']], ['campaign.faults.network_heal',
                                                           'campaign.faults.validator_host',
                                                           'campaign.network.attempts', 'campaign.network.successes'])

    def test_a_drill_record_must_verify_against_the_approved_objective(self):
        with self.assertRaisesRegex(results.Failed, 'restore drill record does not verify.*not the approved'):
            summarize(restores=[rebuild(3600, target=8 * 3600)])
        edited = json.loads(host_loss(600))
        edited['observations']['instants']['first_block_after_halt_at'] -= 300
        with self.assertRaisesRegex(results.Failed, 'recovery drill record does not verify'):
            summarize(recoveries=[json.dumps(edited).encode()])
        # An unfinished drill is no sample.
        unfinished = test_restore_drill.observations()
        del unfinished['instants']['caught_up_at']
        raw = json.dumps(restore_drill.build('dytallix-staging-2', NATIVE, unfinished,
                                             restore_drill.approved_target())).encode()
        with self.assertRaisesRegex(results.Failed, 'no recovery, attack, load run or fault alerts for a campaign KPI'):
            summarize(restores=[raw])

    def test_the_attack_family_counts_its_fewest_type_and_every_success(self):
        summary = results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT,
                                             attack_files=[attack_record(2000), attack_record(1500)])
        self.assertEqual(results.summary_problems(summary), [])
        attempts, successes = (kpi(summary, 'campaign.transactions.attempts'),
                               kpi(summary, 'campaign.transactions.successes'))
        # 3,500 refusals for every type of the seventeen, none admitted.
        self.assertEqual((attempts['value'], attempts['status'], attempts['target']), (3500, 'pass', 3000))
        self.assertEqual((successes['value'], successes['status'], successes['target']), (0, 'pass', None))
        self.assertEqual(summary['date'], '2026-11-04')
        self.assertEqual({s['tool'] for s in summary['sources']}, {'attacks.transactions'})
        failed = results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT,
                                            attack_files=[attack_record(3, admitted={'overspend'})])
        self.assertEqual((kpi(failed, 'campaign.transactions.successes')['value'],
                          kpi(failed, 'campaign.transactions.successes')['status'],
                          kpi(failed, 'campaign.transactions.attempts')['status']), (1, 'fail', 'fail'))

    def test_a_complete_family_passes_only_once_every_type_reaches_the_count(self):
        for count, status in ((2999, 'in_progress'), (3000, 'pass')):
            summary = results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT,
                                                 attack_files=[attack_record(count)])
            self.assertEqual([kpi(summary, f'campaign.transactions.{k}')['status'] for k in ('attempts', 'successes')],
                             [status, status])
        # A type not built counts zero, so the family waits for it.
        with unittest.mock.patch.dict(attacks.REASONS, {'too_many_actions': None}):
            summary = results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT,
                                                 attack_files=[attack_record(3000)])
            self.assertEqual(kpi(summary, 'campaign.transactions.attempts')['value'], 0)

    def test_the_endpoint_family_counts_like_the_transaction_family(self):
        rows = test_endpoints.sample_rows(30)
        record = endpoints.build_record('dytallix-staging-2', NATIVE, ENGINE, rows, 30, '2026-11-04T08:00:00Z',
                                        '2026-11-04T08:30:00Z')
        summary = results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT, attack_files=[
            (json.dumps(record).encode(), endpoints.rows_bytes(rows))])
        self.assertEqual(results.summary_problems(summary), [])
        attempts = kpi(summary, 'campaign.endpoint_rpc.attempts')
        # Three types refused 30 times each; the rest not run or not built yet.
        self.assertEqual((attempts['value'], attempts['status'], attempts['target']), (0, 'in_progress', 30))
        self.assertEqual(kpi(summary, 'campaign.endpoint_rpc.successes')['value'], 0)
        self.assertEqual({s['tool'] for s in summary['sources']}, {'attacks.endpoint_rpc'})
        served = test_endpoints.sample_rows(30, admitted='connection_flood_address')
        record = endpoints.build_record('dytallix-staging-2', NATIVE, ENGINE, served, 30, '2026-11-04T08:00:00Z',
                                        '2026-11-04T08:30:00Z')
        failed = results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT, attack_files=[
            (json.dumps(record).encode(), endpoints.rows_bytes(served))])
        self.assertEqual((kpi(failed, 'campaign.endpoint_rpc.successes')['value'],
                          kpi(failed, 'campaign.endpoint_rpc.successes')['status']), (1, 'fail'))

    def test_an_attack_record_refused_otherwise_or_edited_is_not_published(self):
        with self.assertRaisesRegex(results.Failed, 'refused for another reason .*fix its reason'):
            results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT,
                                       attack_files=[attack_record(2, other={'wrong_network'})])
        raw, rows = attack_record(2)
        with self.assertRaisesRegex(results.Failed, 'an attack record does not verify'):
            results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT, attack_files=[(raw, rows + b'{}\n')])

    def test_a_summary_needs_a_result_kpi_and_a_fault_recovery(self):
        with self.assertRaisesRegex(results.Failed, 'no recovery, attack, load run or fault alerts for a campaign KPI'):
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


class Performance(unittest.TestCase):
    """campaign.performance from the load runs' evidence (load runs only)."""

    def load_record(self, per_block=1, interval=5, heights=6, late=0):
        import test_load
        rows = [test_load.tx_row('run', 'send_drt', block=11, submit=10) for _ in range(per_block)]
        rows += [test_load.tx_row('run', 'send_drt', block=14, submit=10) for _ in range(late)]
        raw = load.rows_jsonl(rows)
        ledger = load.ledger(rows, 10, 10)
        charged = sum(int(e['charge_udrt']) for e in ledger['entries'])
        times = {str(10 + h): f'2026-11-09T00:{(h * interval) // 60:02d}:{(h * interval) % 60:02d}Z'
                 for h in range(heights)}
        settings = dict(test_load.SETTINGS, max_refused=None)
        record = load.evidence(test_load.CHAIN, settings, {}, raw, ledger, test_load.supply(0, 10),
                               test_load.supply(charged, 16), times, {}, {}, '2026-11-09T00:00:00Z',
                               '2026-11-09T01:00:00Z')
        return json.dumps(record).encode(), raw

    def kpis(self, loads):
        return {k['id']: k for k in results.performance_kpis(*results.load_samples(loads)[:2])}

    def test_a_short_run_records_values_in_progress(self):
        found = self.kpis([self.load_record()])
        self.assertEqual(found['campaign.performance.block_interval_p50']['value'], 5.0)
        self.assertEqual({k['status'] for k in found.values()}, {'in_progress'})
        self.assertEqual(found['campaign.performance.inclusion_within_2_blocks']['value'], 100.0)
        self.assertEqual(found['campaign.performance.response_time_p99']['value'], 0.02)
        self.assertTrue(all(k['target'] is None for k in found.values()))

    def test_bounds_fail_at_once(self):
        found = self.kpis([self.load_record(interval=7)])
        self.assertEqual(found['campaign.performance.block_interval_p50']['status'], 'fail')
        found = self.kpis([self.load_record(per_block=98, late=2)])
        self.assertEqual(found['campaign.performance.inclusion_within_2_blocks']['status'], 'fail')

    def test_half_capacity_and_a_day_pass(self):
        found = self.kpis([self.load_record(per_block=95)])
        self.assertEqual(found['campaign.performance.inclusion_within_2_blocks']['status'], 'pass')
        self.assertEqual(found['campaign.performance.response_time_p99']['status'], 'pass')
        with mock.patch.object(results, 'DAY_INTERVALS', 5):
            found = self.kpis([self.load_record(per_block=95)])
        for name in ('block_interval_p50', 'block_interval_p99', 'block_interval_p95'):
            self.assertEqual(found[f'campaign.performance.{name}']['status'], 'pass', name)
        self.assertEqual(found['campaign.performance.block_interval_p99_9']['status'], 'in_progress')

    def test_samples_pool_over_runs_and_an_edited_run_is_refused(self):
        a, b = self.load_record(), self.load_record(interval=4)
        found = self.kpis([a, b])
        self.assertEqual(found['campaign.performance.block_interval_p50']['samples'], 10)
        record = json.loads(a[0])
        record['summary']['inclusion']['within_2'] = 0
        with self.assertRaises(results.Failed):
            results.load_samples([(json.dumps(record).encode(), a[1])])

    def test_the_kpis_pass_the_page_checks(self):
        summary = {'schema': results.SCHEMA, 'scope': 'campaign', 'date': '2026-11-09',
                   'kpis': list(self.kpis([self.load_record()]).values())}
        self.assertEqual(results.summary_problems(summary), [])


class Network(unittest.TestCase):
    """campaign.network from the fault records' network scenarios; E03's
    peer types count zero until its evidence is read (P01, 9 November 2026)."""

    def scenario(self, name, **over):
        base = row(name, 20)
        base['effect'] = {'installed': True} if name == 'partition' else \
            {'qdisc': 'netem delay 400ms'} if name in ('latency', 'loss') else \
            {'closed': ['a:26656 b:1'], 'after': []}
        base.update(over)
        return base

    def network(self, rows):
        summary = summarize(record(rows))
        return kpi(summary, 'campaign.network.attempts'), kpi(summary, 'campaign.network.successes')

    def test_the_peer_types_keep_the_family_in_progress(self):
        rows = [self.scenario(n) for n in results.NETWORK_FAULTS for _ in range(30)]
        attempts, successes = self.network(rows)
        self.assertEqual((attempts['status'], attempts['value'], attempts['target']), ('in_progress', 0, 30))
        self.assertEqual((successes['value'], attempts['samples']), (0, 120))
        # With E03's two types at 30 too, the family would pass.
        refused = [r for r in rows]
        with unittest.mock.patch.object(results, 'NETWORK_TYPES', results.NETWORK_FAULTS):
            self.assertEqual(results.network_kpis(refused)[0]['status'], 'pass')

    def test_lost_history_or_no_recovery_is_a_success(self):
        _, successes = self.network([self.scenario('partition', recovered_height=90)])
        self.assertEqual((successes['value'], successes['status']), (1, 'fail'))
        lost = self.scenario('latency', recovered_at=None, recovered_height=None)
        self.assertEqual(results.network_outcome(dict(lost, status='FAIL')), 'success')

    def test_what_is_no_attempt(self):
        self.assertIsNone(results.network_outcome(dict(self.scenario('loss'), status='INCONCLUSIVE')))
        self.assertIsNone(results.network_outcome(dict(self.scenario('loss', effect={'qdisc': 'pfifo'}),
                                                       status='FAIL')))
        broken = self.scenario('peer_drop', command={'command': 'fault-peer-drop', 'args': {}, 'status': 'FAILED'})
        self.assertIsNone(results.network_outcome(dict(broken, status='FAIL')))
        # A slow recovery fails the fault KPI but is a refused attempt here.
        self.assertEqual(results.network_outcome(dict(self.scenario('partition'), status='FAIL')), 'refused')

    def test_a_process_kill_is_not_a_network_attempt(self):
        attempts, _ = self.network([self.scenario('partition'), row('process_kill', 20)])
        self.assertEqual(attempts['samples'], 1)


class FaultAlerts(unittest.TestCase):
    def alerts(self, **decided):
        import fault_alerts
        import test_fault_alerts
        with unittest.mock.patch.dict(fault_alerts.EXPECTED, decided):
            raw = json.dumps(test_fault_alerts.build()).encode()
            return raw, results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT, alert_files=[raw])

    def test_undecided_alerts_keep_the_kpi_in_progress_with_no_value(self):
        _, summary = self.alerts()
        self.assertEqual(kpi(summary, 'campaign.faults.alerts'),
                         {'id': 'campaign.faults.alerts', 'status': 'in_progress', 'value': None, 'unit': '%',
                          'samples': 0, 'target': None})
        self.assertEqual(summary['date'], '2026-11-10')
        self.assertEqual(summary['sources'][0]['tool'], 'fault_alerts')
        self.assertEqual(results.summary_problems(summary), [])

    def test_a_missed_alert_fails_and_all_raised_never_passes_yet(self):
        import test_fault_alerts
        _, summary = self.alerts(**test_fault_alerts.DECIDED)
        found = kpi(summary, 'campaign.faults.alerts')
        self.assertEqual((found['status'], found['value'], found['samples']), ('in_progress', 100.0, 4))
        missed = results.alert_kpi(['PASS', 'FAIL', 'PASS', 'PASS'])
        self.assertEqual((missed['status'], missed['value']), ('fail', 75.0))

    def test_an_edited_alerts_record_is_refused(self):
        raw, _ = self.alerts()
        edited = json.loads(raw)
        edited['faults'][1]['status'] = 'PASS'
        with self.assertRaisesRegex(results.Failed, 'fault alerts record does not verify'):
            results.summarize_campaign([], NATIVE, ENGINE, TIMINGS, RELEASE, COMMIT,
                                       alert_files=[json.dumps(edited).encode()])
