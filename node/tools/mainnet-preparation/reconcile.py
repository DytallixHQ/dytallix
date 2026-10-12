#!/usr/bin/env python3
"""The reconciliation report (simulation plan, T6): at a height, every node's
block hash, application hash and supply buckets compared; the supply rules on
every reading (DRT total = genesis + emitted - burned, the buckets summing to
their totals, no fee left unburned, DGT issued unchanged from genesis); fees
against burns over a recorded ledger; the tracked accounts summed against the
liquid totals; and `dytallix-state-check` on a stopped copy of one node's
database.

  reconcile.py verify --evidence FILE --genesis NATIVE_GENESIS

The plan's integrity criterion is all or nothing: any difference is FAIL and
stops the run. A reading that could not be made (no common height, no quiet
window for the accounts, no recorded application hash at the checked height)
is INCONCLUSIVE, which no run may count as a pass. Only PASS exits 0;
success is never inferred from a process exiting.

The readings come through a host transport: anything with `run(args,
stdin=None, timeout=600, check=True) -> (code, out, err)` and `read(path) ->
bytes`. `LocalHost` is the host this process runs on; the staging network's
guest agent (`staging_network.Agent`) is another; the staging agent (T2) will
be a third. Clients read the public node through the release's CLI over its
client channel.

Each host's engine reports its newest block hash and the application hash in
that block's header, which is the application's hash after the block before;
its metrics file holds the application's supply at its own height. The
client's status view and proven balances give the application's hash at
their height. The evidence keeps every reading, so `verify` re-evaluates it
from the file alone and checks its digest.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'host'))
import monitor  # noqa: E402

SCHEMA = 'dytallix.reconciliation.v1'
LEDGER_SCHEMA = 'dytallix.reconcile-ledger.v1'
METRICS = '/var/lib/dytallix/metrics/dytallix-app.prom'
APPDB = '/var/lib/dytallix/node/appdb'
UNIT = 'dytallix-node'
OPERATOR_STATUS = ('sh', '-c', 'exec /opt/dytallix/*/bin/dytallix-operator-rpc --home /var/lib/dytallix/node status')
RELEASE_BIN = re.compile(r'/opt/dytallix/([A-Za-z0-9._-]+)/bin/')
DRT = ('genesis', 'emitted', 'burned', 'total', 'liquid', 'withheld_fees')
DGT = ('issued', 'liquid', 'staked', 'pending_bonded', 'unbonding', 'penalty_reserve', 'governance_escrow')
PASS, FAIL, INCONCLUSIVE = 'PASS', 'FAIL', 'INCONCLUSIVE'


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


# Readings

def metrics_supply(text):
    """The application's height and supply buckets from its metrics file."""
    samples = monitor.parse_metrics(text)
    height = monitor.value(samples, 'dytallix_app_height')
    if height is None:
        return None
    supply = {'udrt': {}, 'udgt': {}}
    for (name, labels), amount in samples.items():
        for denomination in supply:
            if name == f'dytallix_app_supply_{denomination}':
                supply[denomination][dict(labels)['bucket']] = str(amount)
    if not all(b in supply['udrt'] for b in DRT) or not all(b in supply['udgt'] for b in DGT):
        return None
    return int(height), supply


def view_supply(view):
    """The same buckets from a status view (`supply` and `dgt_supply`)."""
    drt, dgt = view['supply'], view['dgt_supply']
    udrt = {b: str(int(drt[b])) for b in DRT}
    udrt.update({f'pool_{k}': str(int(v)) for k, v in drt.get('pools', {}).items()})
    udgt = {b: str(int(dgt[b] or 0)) for b in DGT}
    return {'udrt': udrt, 'udgt': udgt}


def engine_status(raw):
    """(height, block hash, header application hash) from the operator status."""
    info = json.loads(raw)['result']['sync_info']
    return int(info['latest_block_height']), info['latest_block_hash'].lower(), info['latest_app_hash'].lower()


def genesis_totals(native_genesis):
    """The DRT and DGT the native genesis allocates: the application's DRT
    genesis counter and the DGT issued are their account sums."""
    accounts = native_genesis['accounts']
    return {'udrt': str(sum(int(a['balances'].get('udrt', 0)) for a in accounts)),
            'udgt': str(sum(int(a['balances'].get('udgt', 0)) for a in accounts))}


def supply_rules(supply, genesis):
    """Every rule a supply reading breaks (supply mismatch runbook)."""
    d = {k: int(v) for k, v in supply['udrt'].items()}
    g = {k: int(v) for k, v in supply['udgt'].items()}
    broken = []
    if d['total'] != d['genesis'] + d['emitted'] - d['burned']:
        broken.append(f'DRT total {d["total"]} is not genesis + emitted - burned '
                      f'({d["genesis"] + d["emitted"] - d["burned"]})')
    pools = sum(v for k, v in d.items() if k.startswith('pool_'))
    if d['liquid'] + d['withheld_fees'] + pools != d['total']:
        broken.append(f'DRT liquid + withheld + pools ({d["liquid"] + d["withheld_fees"] + pools}) '
                      f'is not the total {d["total"]}')
    if d['withheld_fees'] != 0:
        broken.append(f'{d["withheld_fees"]} udrt of fees withheld after a commit; every fee is burned')
    if d['genesis'] != int(genesis['udrt']):
        broken.append(f'DRT genesis {d["genesis"]} is not the native genesis allocation {genesis["udrt"]}')
    held = sum(g[b] for b in DGT if b != 'issued')
    if held != g['issued']:
        broken.append(f'DGT buckets sum to {held}, not the issued {g["issued"]}')
    if g['issued'] != int(genesis['udgt']):
        broken.append(f'DGT issued {g["issued"]} changed from the genesis allocation {genesis["udgt"]}')
    return broken


# Evaluation

class Checks:
    def __init__(self):
        self.rows = []

    def add(self, name, status, detail):
        self.rows.append({'check': name, 'status': status, 'detail': detail})

    def status(self):
        found = {r['status'] for r in self.rows}
        return FAIL if FAIL in found else INCONCLUSIVE if INCONCLUSIVE in found or not self.rows else PASS


def app_hashes(observations):
    """Every application hash recorded, by height, with its sources: a
    header at H carries the hash after H - 1."""
    found = {}
    for label, host in observations['hosts'].items():
        for height, engine in host['engine'].items():
            found.setdefault(int(height) - 1, {})[f'{label} engine header {height}'] = engine['header_app_hash']
    for height, view in observations.get('views', {}).items():
        found.setdefault(int(height), {})[f'client status view {height}'] = view['app_hash'].lower()
    for balance in (observations.get('accounts') or {}).get('balances', []):
        found.setdefault(int(balance['height']), {})[f'client balance {balance["address"]}'] = balance['app_hash'].lower()
    return found


def common_height(observations):
    """The newest height read from every host's engine and metrics (and the
    client's status view, if one was read)."""
    sets = []
    for host in observations['hosts'].values():
        sets += [set(host['engine']), set(host['metrics'])]
    if observations.get('views') is not None and observations.get('client'):
        sets.append(set(observations['views']))
    common = set.intersection(*sets) if sets else set()
    return max(map(int, common)) if common else None


def evaluate(observations, genesis):
    checks = Checks()
    hosts = observations['hosts']
    height = common_height(observations)
    if height is None:
        checks.add('common_height', INCONCLUSIVE, 'no height was read from every node')
    else:
        checks.add('common_height', PASS, f'height {height} read from {", ".join(sorted(hosts))}')

    # Integrity: one block and one application hash per height, on every node.
    heights = {h for host in hosts.values() for h in host['engine']}
    split = []
    for h in sorted(heights, key=int):
        blocks = {label: host['engine'][h]['block_hash'] for label, host in hosts.items() if h in host['engine']}
        if len(set(blocks.values())) > 1:
            split.append(f'height {h}: {blocks}')
    checks.add('block_hashes', FAIL if split else PASS,
               '; '.join(split) if split else f'one block hash at each of {len(heights)} heights')
    differ = {h: s for h, s in app_hashes(observations).items() if len(set(s.values())) > 1}
    checks.add('app_hashes', FAIL if differ else PASS,
               '; '.join(f'height {h}: {s}' for h, s in sorted(differ.items())) if differ
               else 'one application hash at every height read')

    # Supply: the same buckets on every node at a height, and every rule.
    readings = [(f'{label} metrics', h, s) for label, host in hosts.items() for h, s in host['metrics'].items()]
    readings += [('client status view', h, v['supply']) for h, v in observations.get('views', {}).items()]
    by_height, differ = {}, []
    for source, h, supply in readings:
        by_height.setdefault(int(h), {})[source] = supply
    for h, sources in sorted(by_height.items()):
        if len({canonical(s) for s in sources.values()}) > 1:
            differ.append(f'height {h}: {sorted(sources)} differ')
    checks.add('supply_equal', FAIL if differ else PASS,
               '; '.join(differ) if differ else 'the same buckets wherever two readings share a height')
    broken = [f'{source} at {h}: {b}' for source, h, s in readings for b in supply_rules(s, genesis)]
    since = observations.get('since')
    if since:
        broken += [f'the earlier reading at {since["height"]}: {b}' for b in supply_rules(since['supply'], genesis)]
    checks.add('supply_rules', FAIL if broken else PASS,
               '; '.join(broken) if broken else f'every rule holds in {len(readings)} readings')

    # Emission and burning only grow, on each node and since the last reading.
    regress = []
    series = [(f'{label}', sorted(((int(h), s) for h, s in host['metrics'].items())))
              for label, host in hosts.items()]
    if since and height is not None:
        first = next(iter(hosts.values()))['metrics'].get(str(height))
        if first:
            series.append(('since the earlier reading', [(int(since['height']), since['supply']), (height, first)]))
    for source, points in series:
        for (h0, s0), (h1, s1) in zip(points, points[1:]):
            for bucket in ('emitted', 'burned'):
                if int(s1['udrt'][bucket]) < int(s0['udrt'][bucket]):
                    regress.append(f'{source}: {bucket} fell from {s0["udrt"][bucket]} at {h0} to '
                                   f'{s1["udrt"][bucket]} at {h1}')
    if since and height is not None and height <= int(since['height']):
        regress.append(f'height {height} is not after the earlier reading at {since["height"]}')
    checks.add('monotonic', FAIL if regress else PASS, '; '.join(regress) if regress else 'emitted and burned only grow')

    accounts(checks, observations)
    fees(checks, observations, height)
    state_check(checks, observations)
    return height, checks


def accounts(checks, observations):
    """The tracked accounts' proven balances against the liquid totals. The
    totals move only with a transaction, so the readings count only inside a
    window in which the client's status view saw neither liquid total move."""
    recorded = observations.get('accounts')
    if not recorded:
        return
    before, after, balances = recorded['before'], recorded['after'], recorded['balances']
    liquid = lambda view: (view['supply']['udrt']['liquid'], view['supply']['udgt']['liquid'])  # noqa: E731
    outside = [b['address'] for b in balances if not int(before['height']) <= int(b['height']) <= int(after['height'])]
    if liquid(before) != liquid(after) or outside:
        checks.add('accounts', INCONCLUSIVE, f'no quiet window: liquid {liquid(before)} at {before["height"]}, '
                   f'{liquid(after)} at {after["height"]}; read outside it: {outside}')
        return
    missing = [b['address'] for b in balances if not b['account_exists']]
    udrt, udgt = sum(int(b['udrt']) for b in balances), sum(int(b['udgt']) for b in balances)
    wrong = []
    if missing:
        wrong.append(f'tracked accounts missing: {missing}')
    if str(udrt) != liquid(after)[0]:
        wrong.append(f'{len(balances)} tracked accounts hold {udrt} udrt, the liquid total is {liquid(after)[0]}')
    if str(udgt) != liquid(after)[1]:
        wrong.append(f'{len(balances)} tracked accounts hold {udgt} udgt, the liquid total is {liquid(after)[1]}')
    checks.add('accounts', FAIL if wrong else PASS, '; '.join(wrong) if wrong else
               f'{len(balances)} tracked accounts hold the liquid {udrt} udrt and {udgt} udgt '
               f'(heights {before["height"]} to {after["height"]})')


def fees(checks, observations, height):
    """Fees against burns (fees v1): every fee is burned when charged and each
    created account burns the creation fee, so over a complete ledger of the
    committed transactions since the earlier reading the burned counter grows
    by exactly their sum."""
    since, ledger = observations.get('since'), observations.get('ledger')
    if not ledger:
        return
    if not since or height is None:
        checks.add('fees_and_burns', INCONCLUSIVE, 'a ledger needs the earlier reading and a common height')
        return
    if ledger.get('schema') != LEDGER_SCHEMA or int(ledger['from_height']) != int(since['height']):
        checks.add('fees_and_burns', FAIL, f'the ledger does not start at the earlier reading ({since["height"]})')
        return
    now = next(iter(observations['hosts'].values()))['metrics'][str(height)]
    burned = int(now['udrt']['burned']) - int(since['supply']['udrt']['burned'])
    charged = sum(int(e['charge_udrt']) for e in ledger['entries'])
    created = sum(int(e.get('created_accounts', 0)) for e in ledger['entries'])
    expected = charged + created * int(ledger['account_creation_fee_udrt'])
    checks.add('fees_and_burns', PASS if burned == expected else FAIL,
               f'{burned} udrt burned from {since["height"]} to {height}; {len(ledger["entries"])} transactions '
               f'charged {charged} and created {created} accounts, {expected} expected')


def state_check(checks, observations):
    recorded = observations.get('state_check')
    if not recorded:
        return
    result = recorded['result']
    if result.get('result') != 'pass':
        checks.add('state_check', FAIL, f'{recorded["host"]}: {result.get("result")} '
                   f'{result.get("class")}: {result.get("error")}')
        return
    height, value = int(result['height']), result['app_hash'].lower()
    known = app_hashes(observations).get(height, {})
    if not known:
        checks.add('state_check', INCONCLUSIVE, f'{recorded["host"]} passed at {height}, '
                   'but no application hash was read for that height')
    elif set(known.values()) != {value}:
        checks.add('state_check', FAIL, f'{recorded["host"]} passed at {height} with {value}; the network: {known}')
    else:
        checks.add('state_check', PASS, f'{recorded["host"]}: every startup check passes at {height}, '
                   f'with the network\'s application hash {value}')


def evidence(chain_id, observations, genesis_bytes, started, finished):
    genesis = genesis_totals(json.loads(genesis_bytes))
    height, checks = evaluate(observations, genesis)
    body = {'schema': SCHEMA, 'chain_id': chain_id, 'status': checks.status(), 'height': height,
            'started': started, 'finished': finished,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes), **genesis},
            'checks': checks.rows, 'observations': observations}
    if height is not None:
        first = next(iter(observations['hosts'].values()))
        body['block_hash'] = first['engine'][str(height)]['block_hash']
        body['supply'] = first['metrics'][str(height)]
    return dict(body, sha256=digest(canonical(body)))


def verify(record, genesis_bytes):
    """Re-evaluate recorded evidence from its readings; any edit shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if body['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    height, checks = evaluate(body['observations'], genesis_totals(json.loads(genesis_bytes)))
    if (height, checks.rows, checks.status()) != (body['height'], body['checks'], body['status']):
        raise Failed('the recorded checks do not follow from the recorded readings')
    return body['status']


# Collection

class LocalHost:
    """The host this process runs on (the CI validator; an operator at a
    host's console)."""

    def run(self, args, stdin=None, timeout=600, check=True):
        result = subprocess.run(list(args), input=stdin, capture_output=True, text=True, timeout=timeout)
        if check and result.returncode != 0:
            raise Failed(f'{" ".join(args)} exited {result.returncode}: {result.stderr.strip()}')
        return result.returncode, result.stdout, result.stderr

    def read(self, path):
        return Path(path).read_bytes()


class Client:
    """The release's CLI pinned to the public node's client channel."""

    def __init__(self, cli, env, scratch):
        self.cli, self.env, self.scratch = str(cli), env, Path(scratch)
        self.saved = 0

    def call(self, *args):
        result = subprocess.run([self.cli, *map(str, args)], capture_output=True, text=True, env=self.env, timeout=120)
        if result.returncode != 0:
            raise Failed(f'dytallix {args[0]} failed: {result.stderr.strip() or result.stdout.strip()}')
        return result.stdout

    def status(self):
        self.saved += 1
        path = self.scratch / f'reconcile-status-{self.saved}.json'
        self.call('control', 'status', '--out', path)
        return json.loads(path.read_bytes())

    def balance(self, address):
        return json.loads(self.call('balance', address))


def reading(view):
    return {'height': int(view['height']), 'app_hash': view['app_hash'], 'supply': view_supply(view)}


class Collector:
    def __init__(self, hosts, client=None, clock=time.monotonic, sleep=time.sleep, say=print):
        self.hosts, self.client, self.clock, self.sleep, self.say = hosts, client, clock, sleep, say
        self.observations = {'hosts': {label: {'engine': {}, 'metrics': {}} for label in hosts},
                             'views': {}, 'client': client is not None}

    def sample(self, labels=None):
        """One reading of each node. A node that cannot be read (stopped,
        starting, its transport failing) gives no reading this round; a missing
        reading can only make the result INCONCLUSIVE, never a pass."""
        for label in labels or self.hosts:
            host, record = self.hosts[label], self.observations['hosts'][label]
            try:
                code, raw, _ = host.run(list(OPERATOR_STATUS), check=False)
                if code == 0:
                    height, block, app = engine_status(raw)
                    record['engine'][str(height)] = {'block_hash': block, 'header_app_hash': app}
            except Exception as error:  # noqa: BLE001 - any transport's failure
                self.say(f'{label}: no engine reading ({error})')
            try:
                found = metrics_supply(host.read(METRICS).decode('utf-8', errors='replace'))
            except Exception:  # noqa: BLE001 - a missing file, any transport's failure
                found = None
            if found:
                record['metrics'][str(found[0])] = found[1]
        if self.client and labels is None:
            try:
                view = reading(self.client.status())
            except Failed as error:
                self.say(f'client: no status view ({error})')
                return
            self.observations['views'][str(view['height'])] = {'app_hash': view['app_hash'], 'supply': view['supply']}

    def until_common(self, timeout, period=1.0):
        deadline = self.clock() + timeout
        while True:
            self.sample()
            height = common_height(self.observations)
            if height is not None:
                return height
            if self.clock() > deadline:
                return None
            self.sleep(period)

    def burst(self, labels, seconds, period=0.5):
        deadline = self.clock() + seconds
        while self.clock() < deadline:
            self.sample(labels)
            self.sleep(period)

    def accounts(self, addresses, attempts=3):
        for _ in range(attempts):
            before = reading(self.client.status())
            balances = []
            for address in addresses:
                b = self.client.balance(address)
                balances.append({k: b[k] for k in ('address', 'height', 'account_exists', 'udrt', 'udgt', 'app_hash')})
            after = reading(self.client.status())
            self.observations['accounts'] = {'before': before, 'after': after, 'balances': balances}
            quiet = (before['supply']['udrt']['liquid'], before['supply']['udgt']['liquid']) == \
                (after['supply']['udrt']['liquid'], after['supply']['udgt']['liquid'])
            if quiet:
                return True
            self.say(f'== the liquid totals moved between {before["height"]} and {after["height"]}; reading again')
        return False

    def state_check(self, label, scratch, wait=15):
        """Stop the node, copy its database, start it again and check the copy
        with the release's own startup checks; meanwhile the other nodes' headers
        record the application hash at the stopped height."""
        host = self.hosts[label]
        others = [other for other in self.hosts if other != label]
        _, unit, _ = host.run(['systemctl', 'show', '-p', 'ExecStart', '--value', f'{UNIT}.service'])
        found = RELEASE_BIN.search(unit)
        if not found:
            raise Failed(f'{label}: no release in the unit\'s ExecStart: {unit.strip()}')
        release = found.group(1)
        self.burst(list(self.hosts), 6)
        self.say(f'== {label}: stop the node and copy its database')
        host.run(['systemctl', 'stop', f'{UNIT}.service'], timeout=120)
        try:
            self.burst(others, wait)
            host.run(['rm', '-rf', scratch])
            host.run(['cp', '-a', APPDB, scratch], timeout=1800)
        finally:
            host.run(['systemctl', 'start', f'{UNIT}.service'], timeout=120)
        try:
            code, out, err = host.run([f'/opt/dytallix/{release}/bin/dytallix-state-check',
                                       '--config', f'/etc/dytallix/{release}/application-config.json',
                                       '--genesis', f'/etc/dytallix/{release}/native-genesis.json',
                                       '--db', scratch], timeout=1800, check=False)
        finally:
            host.run(['rm', '-rf', scratch], check=False)
        try:
            result = json.loads(out)
        except ValueError:
            result = {'result': 'unreadable', 'class': None, 'error': f'exit {code}: {out.strip()} {err.strip()}'}
        self.observations['state_check'] = {'host': label, 'release': release, 'exit': code, 'result': result}
        self.say(f'== {label} state check: {json.dumps(result)}')
        return result


def collect(chain_id, hosts, genesis_bytes, client=None, addresses=(), since=None, ledger=None,
            state_check_host=None, scratch='/var/tmp/dytallix-reconcile-appdb', timeout=180, say=print):
    """Read every node, then evaluate: the evidence, whatever its status."""
    started = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    collector = Collector(hosts, client, say=say)
    collector.until_common(timeout)
    if client and addresses:
        collector.accounts(addresses)
    if state_check_host:
        collector.state_check(state_check_host, scratch)
        collector.until_common(timeout)
    observations = collector.observations
    if since is not None:
        observations['since'] = since
    if ledger is not None:
        observations['ledger'] = ledger
    finished = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    return evidence(chain_id, observations, genesis_bytes, started, finished)


def since_reading(record):
    """The earlier reading a ledger starts from: an earlier reconciliation or a
    saved status view."""
    if record.get('schema') == SCHEMA:
        return {'height': int(record['height']), 'supply': record['supply']}
    view = reading(record)
    return {'height': view['height'], 'supply': view['supply']}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    v.add_argument('--genesis', type=Path, required=True, help='native-genesis.json')
    args = parser.parse_args()
    try:
        status = verify(json.loads(args.evidence.read_bytes()), args.genesis.read_bytes())
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'reconcile: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
