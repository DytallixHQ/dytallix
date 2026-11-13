"""The flood family from the load machines (T4 on R03's machines) without a
chain or a machine: the plan's channel arithmetic, a whole campaign where
each worker is a thread with its own address over a stand-in sentry whose
queue holds `bound` transactions, the record floods.py verify accepts, a
worker that fails, rows that do not match their reply, and the files and
commands the machines get. Nothing here rents or reaches a machine."""
import ast
import base64
import json
from pathlib import Path
import shutil
import tempfile
import threading
import time
import unittest
from types import SimpleNamespace
from unittest import mock

import flood_workers as fw
import floods
import load
import load_workers as lw
from test_load_workers import make_fleet

NATIVE, ENGINE = b'{"native": 1}', b'{"engine": 1}'
BOUND = 5
HONEST = (16, 16, [130, 134], [], 0.4)


class Sentry:
    """The chain as the client channel shows it: a head, and a queue that
    takes `bound` transactions and empties when a block comes."""

    def __init__(self, bound):
        self.bound, self.head, self.queued, self.lock = bound, 100, 0, threading.Lock()
        self.due = False

    def block(self):
        with self.lock:
            self.head += 1
            self.queued = 0

    def read(self):
        """The head a worker reads: the block drive's read made due comes
        with the first worker read after it."""
        with self.lock:
            if self.due:
                self.head, self.queued, self.due = self.head + 1, 0, False
            return self.head


class WorkerRpc:
    """A worker's connections."""

    def __init__(self, sentry, connections=2):
        self.sentry, self.connections = sentry, connections

    def one(self, label, method, params):
        return {'response': {'last_block_height': str(self.sentry.read())}}

    def timed(self, label, requests):
        sentry = self.sentry
        with sentry.lock:
            out = [{'response': {'last_block_height': str(sentry.head)}}]
            for _, params in requests[1:]:
                tx = base64.b64decode(params['tx'])
                if sentry.queued < sentry.bound:
                    sentry.queued += 1
                    out.append({'code': 0, 'hash': load.sha256(tx)})
                else:
                    out.append({'code': 20, 'log': floods.QUEUE_REFUSAL})
        return out, 1.0


class Tracker:
    def __init__(self):
        self.rows = []

    def add(self, row):
        self.rows.append(row)

    def drain(self, timeout, interval=1.0):
        for row in self.rows:
            row['status'] = 'committed'
        self.rows = []


class WorkerRun:
    """A worker's Load over its share."""

    def __init__(self, shard, rpc):
        self.rpc, self.tracker = rpc, Tracker()
        self.s = load.Settings(signers=2, block_time=1.0, expiry_blocks=2, poll_interval=0.0)
        self.wallets = {w['label']: load.Wallet(w['label'], w['role'], w['address'], w['account_id'])
                        for w in shard['wallets']}
        self.primed, self.nonce = False, 0

    def role(self, *roles):
        return [w for w in self.wallets.values() if w.role in roles]

    def first_spends(self):
        self.primed = True

    def refresh_profile(self):
        pass

    def sync(self, wallets):
        self.nonce += 1

    def sign(self, row, wallet, actions):
        row.update(id=f'{wallet.label}-{self.nonce}', nonce=self.nonce)
        return row, row['id'].encode()


class ThreadTransport:
    """Each worker a thread with its own address."""

    def __init__(self, root, sentry, fail=(), hammer=None):
        self.root, self.sentry, self.fail = Path(root), sentry, set(fail)
        self.separate, self.poll = True, 0.01
        self.threads, self.stopped, self.closed = {}, False, set()
        self.hammer = hammer or (lambda n, seconds: ({'served': 50, 'errors': 0, 'first_errors': []}, None))

    def folder(self, n):
        return self.root / f'worker-{n}'

    def address(self, n):
        return f'198.51.100.{n + 1}'

    def ship(self, n, shard_dir, files):
        (self.folder(n) / 'out').mkdir(parents=True)
        shutil.copytree(shard_dir, self.folder(n) / 'shard')

    def start(self, n):
        folder = self.folder(n)

        def target():
            try:
                if n in self.fail:
                    raise load.Failed('the machine went away')
                shard = json.loads((folder / 'shard' / 'fleet.json').read_bytes())
                fw.serve(WorkerRun(shard, WorkerRpc(self.sentry)), shard, folder / 'out',
                         lambda: self.closed.add(n), lambda seconds: self.hammer(n, seconds), poll=0.005,
                         sleep=lambda s: time.sleep(min(s, 0.005)))
            except Exception as error:
                lw.write_json(folder / 'out' / 'error.json', {'error': str(error)})
        self.threads[n] = threading.Thread(target=target, daemon=True)
        self.threads[n].start()

    def read(self, n, name):
        path = self.folder(n) / 'out' / name
        return path.read_bytes() if path.exists() else None

    def write(self, n, name, value):
        lw.write_json(self.folder(n) / 'out' / name, value)

    def alive(self, n):
        return self.threads[n].is_alive()

    def stop(self):
        self.stopped = True
        for thread in self.threads.values():
            thread.join(0.5)


class CoordinatorRpc:
    """The founder's connections: after drive reads the head, the next block
    is due."""

    def __init__(self, sentry):
        self.sentry, self.connections = sentry, 2

    def one(self, label, method, params):
        head = self.sentry.read()
        self.sentry.due = True
        return {'response': {'last_block_height': str(head)}}


class CoordinatorRun:
    def __init__(self, fleet, sentry):
        self.s = load.Settings(signers=2, block_time=1.0, expiry_blocks=2, poll_interval=0.0)
        self.sentry = sentry
        self.wallets = {w['label']: load.Wallet(w['label'], w['role'], w['address'], w['account_id'])
                        for w in fleet['wallets']}
        self.funder = load.Wallet('ecosystem-growth', 'funder', 'dyt1funder', 'f' * 64, keystore_name='eg')
        self.funded = self.synced = False

    def role(self, *roles):
        return [w for w in self.wallets.values() if w.role in roles]

    cap = 2 * load.UNIT

    def summary(self, wallet):
        return {'liquid_udrt': str(10**12), 'liquid_udgt': str(10**12)}

    def fund(self, budget=None):
        if budget:
            budget((10**9, 10**9))
        self.funded = True
        for w in self.role('load'):
            w.drt, w.dgt = 200 * load.UNIT, 500 * load.UNIT

    def sync(self, wallets):
        self.synced = True

    def online(self, wallet, args, phase='setup', kind=None):
        self.sentry.block()
        return {'status': 'admitted', 'block_height': self.sentry.head, 'code': 0}

    def follow(self, row):
        pass


class Campaign(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def campaign(self, workers=7, wallets=BOUND + floods.FLOOD_EXTRA, kinds=tuple(floods.REASONS), attempts=2,
                 settings=None, funding=None, **transport):
        fleet = make_fleet(self.root / 'fleet', wallets=wallets, spares=0, distributors=0)
        sentry = Sentry(BOUND)
        run = CoordinatorRun(fleet, sentry)
        self.transport = ThreadTransport(self.root / 'workers', sentry, **transport)
        out = self.root / 'run'
        out.mkdir()
        with mock.patch.object(floods, 'honest_reads', return_value=HONEST):
            rows, summary = fw.campaign(run, CoordinatorRpc(sentry), fleet, self.transport, workers, out, BOUND,
                                        kinds, attempts, settings or load.Settings(), {}, lead=0.0,
                                        show=lambda m: None, sleep=lambda s: time.sleep(min(s, 0.01)), poll=0.005,
                                        funding=funding)
        return fleet, run, rows, summary

    def test_seven_machines_make_a_record_floods_verify_accepts(self):
        fleet, run, rows, summary = self.campaign()
        record = floods.build_record('dytallix-staging-1', NATIVE, ENGINE, rows, 2, BOUND, 'a', 'b')
        self.assertEqual(record['status'], floods.PASS, json.dumps(record['checks']))
        self.assertEqual(floods.verify(json.loads(json.dumps(record)), floods.rows_bytes(rows), NATIVE, ENGINE),
                         floods.PASS)
        # The same limits as the CI record: 7 flooders of 2, 14 of 16.
        self.assertEqual(record['limits'], dict(floods.CHANNEL, queue_max_entries=BOUND, flooders=7,
                                                flood_seconds=floods.FLOOD_SECONDS))
        tx = [r['observed'] for r in rows if r['type'] == 'transaction_flood']
        self.assertEqual([(o['sent'], o['admitted'], o['queue_refused']) for o in tx],
                         [(BOUND + floods.FLOOD_EXTRA, BOUND, floods.FLOOD_EXTRA)] * 2)
        req = [r['observed'] for r in rows if r['type'] == 'request_flood']
        self.assertEqual([(o['flooders'], o['flood_served'], o['honest_served']) for o in req], [(7, 350, 16)] * 2)
        self.assertTrue(run.funded)
        # Each worker held its own share of the flood wallets, and its gateway
        # was stopped before the request floods.
        self.assertEqual([s['address'] for s in summary], [f'198.51.100.{n}' for n in range(1, 8)])
        self.assertEqual(sum(s['wallets'] for s in summary), BOUND + floods.FLOOD_EXTRA)
        self.assertEqual(self.transport.closed, set(range(7)))
        self.assertTrue(self.transport.stopped)
        # The rows name no address.
        self.assertNotIn(b'198.51.100', floods.rows_bytes(rows))

    def test_eight_machines_flood_from_seven_and_the_eighth_idles(self):
        hammered = []

        def hammer(n, seconds):
            hammered.append(n)
            return {'served': 50, 'errors': 0, 'first_errors': []}, None
        fleet, run, rows, summary = self.campaign(workers=8, kinds=('request_flood',), attempts=1, hammer=hammer)
        self.assertEqual(sorted(hammered), list(range(7)))
        self.assertEqual(self.transport.closed, set(range(8)))
        self.assertEqual([r['outcome'] for r in rows if r['type'] == 'request_flood'], ['refused'])

    def test_a_flooder_that_does_not_report_makes_the_flood_again(self):
        def hammer(n, seconds):
            return (None, 'gateway did not start') if n == 3 else ({'served': 5, 'errors': 0}, None)
        fleet, run, rows, summary = self.campaign(kinds=('request_flood',), attempts=1, hammer=hammer)
        self.assertEqual([r['outcome'] for r in rows if r['type'] == 'request_flood'], ['transport'] * 3)
        self.assertIn('gateway did not start', rows[0]['observed']['error'])

    def test_a_worker_that_fails_fails_the_run_and_stops_the_others(self):
        with self.assertRaisesRegex(load.Failed, 'worker 2 failed: the machine went away'):
            self.campaign(fail={2})
        self.assertIn('worker 2 failed', json.loads((self.root / 'run' / 'failure.json').read_bytes())['error'])
        self.assertTrue(self.transport.stopped)

    def test_too_few_wallets_or_machines_start_nothing(self):
        with self.assertRaisesRegex(load.Failed, 'needs 21 load wallets'):
            self.campaign(wallets=20)
        shutil.rmtree(self.root / 'fleet')
        shutil.rmtree(self.root / 'run')
        with self.assertRaisesRegex(load.Failed, 'needs 7 machines'):
            self.campaign(workers=6)
        self.assertFalse((self.root / 'workers').exists())


    def test_the_drive_settings_reach_the_shards_and_the_funding_is_checked_first(self):
        settings = fw.settings_from(SimpleNamespace(expiry_blocks=120, fund_drt=20.0, fund_dgt=1.0, prime_parallel=2,
                                                    retries=4))
        self.assertEqual((settings.expiry_blocks, settings.fund_drt, settings.fund_dgt, settings.resume),
                         (120, 20 * load.UNIT, load.UNIT, True))
        funding = {}
        self.campaign(workers=7, attempts=2, settings=settings, funding=funding)
        shard = json.loads((self.root / 'run' / 'shards' / '00' / 'fleet.json').read_bytes())['settings']
        self.assertEqual((shard['expiry_blocks'], shard['resume'], shard['retries'], shard['prime_parallel']),
                         (120, True, 4, 2))
        self.assertEqual(funding['needs_udrt'], str(10**9))

    def test_too_little_funding_for_the_attempts_starts_nothing(self):
        # R03's fund_drt=10 covers two attempts (2 + 2 x 3 + 6 = 14 > 10 for the control wallet; one is 2 + 3 + 6).
        settings = load.Settings(fund_drt=10 * load.UNIT)
        with self.assertRaisesRegex(load.Failed, 'needs 8 DRT \\(14 for the control'):
            self.campaign(attempts=2, settings=settings)
        self.assertFalse((self.root / 'workers').exists())

    def test_a_funder_short_of_the_funding_is_refused_with_the_shortfall(self):
        run = SimpleNamespace(cap=2 * load.UNIT, funder=None,
                              summary=lambda w: {'liquid_udrt': str(5000 * load.UNIT), 'liquid_udgt': '0'})
        check = fw.preflight(run, load.Settings(), 2, tuple(floods.REASONS), lambda m: None, {})
        with self.assertRaisesRegex(load.Failed, 'short 1000 DRT, 1 DGT'):
            check((6000 * load.UNIT, load.UNIT))


class Pieces(unittest.TestCase):
    def test_the_plan_fits_the_channel_and_the_approved_flooders(self):
        fw.check_plan(7, tuple(floods.REASONS), True)
        fw.check_plan(8, tuple(floods.REASONS), True)
        # Four to six machines can make the transaction floods, not the
        # request floods (an open question, not decided here).
        fw.check_plan(4, ('transaction_flood',), True)
        for count, kinds, separate, why in ((6, ('request_flood',), True, 'needs 7 machines'),
                                            (9, ('transaction_flood',), True, 'exceed the channel'),
                                            (7, ('request_flood',), False, 'sharing this machine'),
                                            (1, ('request_flood',), False, 'its own address')):
            with self.subTest(why=why), self.assertRaisesRegex(load.Failed, why):
                fw.check_plan(count, kinds, separate)

    def test_a_reduced_request_flood_needs_only_its_own_flooders(self):
        fw.check_plan(1, ('request_flood',), True, flooders=1)
        fw.check_plan(2, tuple(floods.REASONS), True, flooders=2)
        with self.assertRaisesRegex(load.Failed, 'needs 3 machines'):
            fw.check_plan(2, ('request_flood',), True, flooders=3)
        for flooders in (0, floods.FLOODERS + 1):
            with self.subTest(flooders=flooders), self.assertRaisesRegex(load.Failed, '--flooders is 1 to'):
                fw.check_plan(7, ('request_flood',), True, flooders=flooders)

    def test_the_target_head_leaves_the_commands_time_to_arrive(self):
        self.assertEqual(fw.target_height(120, 15.0, 5.0), 124)
        self.assertEqual(fw.target_height(120, 0.0, 5.0), 121)
        self.assertEqual(fw.target_height(120, 16.0, 5.0), 125)

    def test_a_worker_waits_for_the_target_head(self):
        heads = iter([118, 119, 120, 121])

        class Rpc:
            def one(self, label, method, params):
                return {'response': {'last_block_height': str(next(heads))}}
        self.assertEqual(fw.wait_height(Rpc(), 120, 10, sleep=lambda s: None), 120)
        # A worker already past the target sends at once; its rows show the head.
        self.assertEqual(fw.wait_height(Rpc(), 120, 10, sleep=lambda s: None), 121)
        clock = iter([0, 5, 11])

        class Stuck:
            def one(self, label, method, params):
                return {'response': {'last_block_height': '7'}}
        with self.assertRaisesRegex(load.Failed, 'stayed at 7'):
            fw.wait_height(Stuck(), 8, 10, sleep=lambda s: None, clock=lambda: next(clock))

    def test_rows_that_are_not_the_replys_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            transport = ThreadTransport(tmp, Sentry(1))
            (transport.folder(0) / 'out').mkdir(parents=True)
            (transport.folder(0) / 'out' / fw.rows_name(3)).write_bytes(b'{"a":1}\n')
            workers = fw.Workers(transport, 1, 0.01)
            self.assertEqual(workers.rows(0, 3, {'rows_sha256': load.sha256(b'{"a":1}\n')}), [{'a': 1}])
            with self.assertRaisesRegex(load.Failed, 'not the ones its reply names'):
                workers.rows(0, 3, {'rows_sha256': load.sha256(b'{"a":2}\n')})

    def test_a_worker_gets_every_module_it_imports_and_nothing_of_the_funder(self):
        def local_imports(name, seen):
            if name in seen:
                return
            seen.add(name)
            tree = ast.parse((fw.HERE / name).read_text())
            for node in ast.walk(tree):
                names = [a.name for a in node.names] if isinstance(node, ast.Import) else \
                    [node.module] if isinstance(node, ast.ImportFrom) and node.module else []
                for module in names:
                    for candidate in (f'{module}.py', f'host/{module}.py'):
                        if (fw.HERE / candidate).exists():
                            local_imports(candidate, seen)
                            break
        needed = set()
        local_imports('flood_workers.py', needed)
        self.assertLessEqual(needed, set(fw.TOOLS))
        for name in fw.TOOLS:
            self.assertTrue((fw.HERE / name).exists(), name)

    def test_the_machines_run_this_files_worker(self):
        local = fw.LocalTransport('/tmp/w', 'validator-1')
        self.assertEqual(local.worker_argv(0)[3:5], ['/tmp/w/worker-00/flood_workers.py', 'work'])
        calls = []

        def run(argv, **kwargs):
            calls.append(argv[-1])
            return mock.Mock(returncode=0, stdout=b'', stderr=b'')
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / 'machines.json').write_text(json.dumps(
                {'machines': [{'name': 'dytallix-load-1-1', 'ipv4': '203.0.113.9'}]}))
            ssh = fw.SshTransport(tmp, 'validator-1', run=run)
            ssh.start(0)
            ssh.alive(0)
            ssh.stop()
        self.assertIn('flood_workers.py work', calls[0])
        self.assertIn('"[f]lood_workers.py work"', calls[1])
        self.assertIn('pkill -f "[f]lood_workers.py work"', calls[2])


if __name__ == '__main__':
    unittest.main()
