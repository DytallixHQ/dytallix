"""Load from several machines (simulation plan, T3) without a chain or a
machine: the shares and the connection budget, a whole run where each worker
is a thread with its own address over test_load's simulated chain, a worker
that fails, and the two transports (SSH against a stand-in ssh that runs its
command here; processes). The staging network runs it for real, each worker
in its own network namespace, in the Host install workflow."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
import unittest

import load
import load_workers as lw
from test_load import ADDRESSES, CHAIN, Chain, ChainRpc, ChainSigner, SimulatedLoad, wallet

CONFIG = {'ordinary': {'fee_profile': {'account_creation_fee_udrt': str(Chain.FEE), 'gas_price': '10'}},
          'lifecycle': {'max_active': 16}, 'governance': {'deposit': {'minimum_deposit_udgt': 10**10}}}


def make_fleet(directory, wallets=12, spares=6, distributors=2):
    directory = Path(directory)
    (directory / 'keys').mkdir(parents=True)
    fleet = {'_dir': str(directory), 'chain_id': CHAIN, 'wallets': []}
    for label, role in load.fleet_labels(wallets, 1, spares, distributors):
        w = wallet(label, role)
        ADDRESSES[w.address] = w.account_id
        (directory / 'keys' / f'{label}.json').write_text('{}')
        fleet['wallets'].append({'label': label, 'role': role, 'address': w.address, 'account_id': w.account_id,
                                 'key_file': f'keys/{label}.json'})
    return fleet


class ThreadTransport:
    """Each worker a thread over the simulated chain, with its own address."""

    def __init__(self, root, chain, fail=()):
        self.root, self.chain, self.fail = Path(root), chain, set(fail)
        self.separate, self.poll = True, 0.01
        self.threads, self.stopped = {}, False

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
                settings = load.Settings(**dict(shard['settings'], governance=False))
                fleet = {'_dir': str(folder / 'shard'), 'chain_id': shard['chain_id'], 'wallets': shard['wallets']}
                run = SimulatedLoad(ChainRpc(self.chain), ChainSigner(), {}, 'dytallix', fleet, folder / 'out',
                                    settings, None, 'validator-1', CONFIG, show=lambda m: None)
                lw.run_worker(run, shard, folder / 'out', poll=0.01)
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
        # The threads are daemons: one still waiting for a start time is left.
        self.stopped = True
        for thread in self.threads.values():
            thread.join(0.5)


class PieceTests(unittest.TestCase):
    def test_the_shares_are_disjoint_and_keep_their_spares(self):
        with tempfile.TemporaryDirectory() as tmp:
            fleet = make_fleet(tmp, wallets=7, spares=3)
        shares = lw.split(fleet, 3)
        self.assertEqual([[w['label'] for w in s] for s in shares],
                         [['load-0001', 'load-0004', 'load-0007', 'spare-0001'],
                          ['load-0002', 'load-0005', 'spare-0002'], ['load-0003', 'load-0006', 'spare-0003']])
        with self.assertRaisesRegex(load.Failed, 'cannot give 4 workers two each'):
            lw.split(fleet, 4)

    def test_the_workers_fit_the_channel(self):
        lw.budget(8, 2, True)
        lw.budget(1, 2, False)
        for workers, connections, separate, why in ((9, 2, True, 'exceed the channel'),
                                                    (4, 3, True, 'may hold 2 channel connections'),
                                                    (2, 2, False, 'sharing this machine'),
                                                    (0, 2, True, 'at least one')):
            with self.subTest(why=why), self.assertRaisesRegex(load.Failed, why):
                lw.budget(workers, connections, separate)

    def test_a_worker_gets_only_the_chain_pin_from_the_home(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            with self.assertRaisesRegex(load.Failed, 'no pinned chain'):
                lw.worker_files('dytallix', home, 'config.json')
            (home / '.dytallix').mkdir()
            (home / '.dytallix' / 'chain.json').write_text('{}')
            (home / '.dytallix' / 'keystore.json').write_text('{"the funder": "stays here"}')
            files = lw.worker_files('dytallix', home, 'config.json')
        self.assertEqual(sorted(k for k in files if k.startswith('home/')), ['home/.dytallix/chain.json'])
        self.assertEqual(set(lw.TOOLS) | {'dytallix', 'application-config.json', 'home/.dytallix/chain.json'},
                         set(files))
        for name in lw.TOOLS:
            self.assertTrue((lw.HERE / name).exists(), name)


class DistributedRunTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def coordinate(self, workers=3, fail=()):
        fleet = make_fleet(self.root / 'fleet')
        funder = wallet('ecosystem-growth', 'funder')
        ADDRESSES[funder.address] = funder.account_id
        chain = Chain(0.05, [funder.account_id])
        settings = load.Settings(rate=60.0, duration=1.0, block_time=0.05, poll_interval=0.02, max_refused=0,
                                 fund_drt=100 * load.UNIT, fund_dgt=100 * load.UNIT)
        out = self.root / 'run'
        out.mkdir()
        run = SimulatedLoad(ChainRpc(chain), ChainSigner(), {}, 'dytallix', fleet, out, settings,
                            {'label': funder.label, 'address': funder.address, 'account_id': funder.account_id},
                            'validator-1', CONFIG, show=lambda m: None)
        self.transport = ThreadTransport(self.root / 'workers', chain, fail)
        record = lw.coordinate(run, fleet, self.transport, workers, out, settings, {}, show=lambda m: None,
                               sleep=lambda s: time.sleep(min(s, 0.05)), lead=0.2, poll=0.01, started='a')
        return fleet, record, out

    def test_the_workers_send_together_and_the_run_is_judged_on_all_rows(self):
        fleet, record, out = self.coordinate()
        self.assertEqual(record['status'], load.PASS, json.dumps(record['checks'], indent=1))
        raw = (out / 'transactions.jsonl').read_bytes()
        self.assertEqual(load.verify(json.loads((out / 'evidence.json').read_bytes()), raw), load.PASS)
        workers = record['window']['workers']
        self.assertEqual([w['shard'] for w in workers], [0, 1, 2])
        self.assertEqual(record['window']['addresses'], ['198.51.100.1', '198.51.100.2', '198.51.100.3'])
        self.assertEqual({w['rate'] for w in workers}, {20.0})
        self.assertEqual(record['settings']['workers'], 3)
        # Every worker sent, each only from and to its own share.
        rows = [json.loads(line) for line in raw.splitlines()]
        share = {w['label']: n for n, s in enumerate(lw.split(fleet, 3)) for w in s}
        run = [r for r in rows if r['phase'] == 'run']
        self.assertEqual({share[r['wallet']] for r in run}, {0, 1, 2})
        for r in run:
            if r.get('recipient'):
                self.assertEqual(share[r['recipient']], share[r['wallet']], r)
        # The funding and the distributors' first spends are the coordinator's;
        # every load wallet primed on its worker.
        setup = [r for r in rows if r['phase'] == 'setup']
        primed = {r['wallet'] for r in setup if r['kind'] == 'first_spend'}
        self.assertEqual(primed, {w['label'] for w in fleet['wallets'] if w['role'] in ('load', 'distributor')})
        self.assertTrue(all(r['wallet'] == 'ecosystem-growth' or r['wallet'].startswith('distributor')
                            for r in setup if r['kind'] == 'fund'))
        # The shares the workers got: their wallets' keys and funding, nothing of the funder's.
        for n in range(3):
            shard = json.loads((self.root / 'workers' / f'worker-{n}' / 'shard' / 'fleet.json').read_bytes())
            self.assertEqual({w['funded_udrt'] for w in shard['wallets'] if w['role'] == 'load'},
                             {str(100 * load.UNIT)})
            self.assertNotIn('ecosystem-growth', {w['label'] for w in shard['wallets']})
            self.assertEqual(sorted(p.name for p in (self.root / 'workers' / f'worker-{n}' / 'shard' / 'keys').iterdir()),
                             sorted(f'{w["label"]}.json' for w in shard['wallets']))

    def test_a_worker_that_fails_fails_the_run_and_stops_the_others(self):
        with self.assertRaisesRegex(load.Failed, 'worker 1 failed: the machine went away'):
            self.coordinate(fail={1})
        self.assertIn('worker 1 failed', json.loads((self.root / 'run' / 'failure.json').read_bytes())['error'])
        self.assertTrue(self.transport.stopped)
        self.assertFalse((self.root / 'run' / 'evidence.json').exists())

    def test_too_many_workers_for_the_channel_rent_nothing(self):
        with self.assertRaisesRegex(load.Failed, 'exceed the channel'):
            self.coordinate(workers=9)
        self.assertFalse((self.root / 'workers').exists())


# A stand-in ssh: records its arguments and runs the remote command here.
FAKE_SSH = textwrap.dedent('''\
    import json, os, subprocess, sys
    with open(os.environ['DYTALLIX_FAKE_SSH_LOG'], 'a') as log:
        log.write(json.dumps(sys.argv[1:]) + '\\n')
    done = subprocess.run(['/bin/sh', '-c', sys.argv[-1]], stdin=sys.stdin.buffer, capture_output=True)
    sys.stdout.buffer.write(done.stdout)
    sys.stderr.buffer.write(done.stderr)
    sys.exit(done.returncode)
''')


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_ssh_ships_reads_and_writes_with_the_runs_key_and_host_file(self):
        run_dir = self.root / 'run'
        run_dir.mkdir()
        (run_dir / 'machines.json').write_text(json.dumps({'machines': [
            {'name': 'dytallix-load-1-1', 'ipv4': '203.0.113.5'}]}))
        ssh = self.root / 'ssh'
        ssh.write_text(f'#!{sys.executable}\n' + FAKE_SSH)
        ssh.chmod(0o755)
        log = self.root / 'ssh.log'
        os.environ['DYTALLIX_FAKE_SSH_LOG'] = str(log)
        self.addCleanup(os.environ.pop, 'DYTALLIX_FAKE_SSH_LOG', None)
        remote = self.root / 'remote'
        transport = lw.SshTransport(run_dir, 'validator-1', remote=str(remote), ssh=str(ssh))
        shard = self.root / 'shard'
        (shard / 'keys').mkdir(parents=True)
        (shard / 'fleet.json').write_text('{}')
        tool = self.root / 'load.py'
        tool.write_text('# tool')
        transport.ship(0, shard, {'load.py': tool, 'home/.dytallix/chain.json': tool})
        self.assertEqual(sorted(str(p.relative_to(remote)) for p in remote.rglob('*')),
                         ['home', 'home/.dytallix', 'home/.dytallix/chain.json', 'load.py', 'out', 'shard',
                          'shard/fleet.json', 'shard/keys'])
        self.assertIsNone(transport.read(0, 'primed.json'))
        transport.write(0, 'start.json', {'start_at': 5})
        self.assertEqual(json.loads(transport.read(0, 'start.json')), {'start_at': 5})
        # A worker is alive while a process runs it, and the shell that asks
        # is not taken for one.
        self.assertFalse(transport.alive(0))
        worker = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)', 'load_workers.py', 'work'])
        self.addCleanup(worker.kill)
        self.assertTrue(transport.alive(0))
        transport.stop()
        worker.wait(10)
        self.assertFalse(transport.alive(0))
        calls = [json.loads(line) for line in log.read_text().splitlines()]
        for argv in calls:
            self.assertEqual(argv[:2], ['-i', str(run_dir / 'id_ed25519')])
            self.assertIn(f'UserKnownHostsFile={run_dir / "known_hosts"}', argv)
            self.assertIn('BatchMode=yes', argv)
            self.assertEqual(argv[-2], 'root@203.0.113.5')
        # The start command, without running it.
        seen = []
        quiet = lw.SshTransport(run_dir, 'validator-1', remote='/root/load',
                                run=lambda argv, **kw: seen.append(argv) or type('Done', (), {'returncode': 0})())
        quiet.start(0)
        self.assertIn('setsid nohup python3 -I -B load_workers.py work --cli ./dytallix --home ./home --shard ./shard '
                      '--out ./out --application-config ./application-config.json --validator validator-1', seen[0][-1])
        self.assertTrue(seen[0][-1].startswith('cd /root/load && '))

    def test_local_workers_run_in_their_folders_and_namespaces(self):
        script = self.root / 'fake_worker.py'
        script.write_text(textwrap.dedent('''\
            import json, os, time
            json.dump({'primed_s': 0.1}, open('out/primed.json', 'w'))
            while not os.path.exists('out/start.json'):
                time.sleep(0.02)
            json.dump({'rows': 0}, open('out/worker.json', 'w'))
        '''))
        transport = lw.LocalTransport(self.root / 'workers', 'validator-1',
                                      argv=lambda n: [sys.executable, '-I', str(script)])
        shard = self.root / 'shard'
        shard.mkdir()
        (shard / 'fleet.json').write_text('{}')
        transport.ship(0, shard, {'load.py': script})
        transport.start(0)
        self.assertEqual(lw.wait_for(transport, 1, 'primed.json', 10, poll=0.02), [{'primed_s': 0.1}])
        transport.write(0, 'start.json', {'start_at': 0})
        self.assertEqual(lw.wait_for(transport, 1, 'worker.json', 10, poll=0.02), [{'rows': 0}])
        transport.stop()
        spaced = lw.LocalTransport(self.root / 'spaced', 'validator-1', ['dytallix-load-0'], ['10.77.0.20'])
        self.assertEqual(spaced.command(0)[:4], ['ip', 'netns', 'exec', 'dytallix-load-0'])
        self.assertEqual(spaced.command(0)[-2:], ['--validator', 'validator-1'])
        self.assertEqual((spaced.separate, spaced.address(0)), (True, '10.77.0.20'))

    def test_a_worker_that_stops_without_its_file_is_reported(self):
        transport = lw.LocalTransport(self.root / 'workers', 'validator-1', argv=lambda n: [sys.executable, '-c', 'pass'])
        shard = self.root / 'shard'
        shard.mkdir()
        transport.ship(0, shard, {})
        transport.start(0)
        transport.processes[0].wait(10)
        with self.assertRaisesRegex(load.Failed, 'worker 0 stopped without primed.json'):
            lw.wait_for(transport, 1, 'primed.json', 10, poll=0.02)


if __name__ == '__main__':
    unittest.main()
