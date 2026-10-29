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
guest agent (`staging_network.Agent`) is another. A console-only host is read
through the staging agent (T2) instead: `collect_through_agent` sends its
read-only reconcile-read to every host for the same minute and evaluates the
readings its sealed results carry. Clients read the public node through the
release's CLI over its client channel.

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
# The running release's binary, which the node unit's ExecStart names. After a
# release switch two releases are installed (the previous one stays until the
# next stage), and a glob over /opt/dytallix would name both (upgrade drill, T8).
NODE_UNIT_FILE = '/etc/systemd/system/dytallix-node.service'
OPERATOR_STATUS = ('sh', '-c', 'exec "$(sed -n "s|^ExecStart=\\(/opt/dytallix/[0-9a-f]*/bin\\)/.*|\\1|p" '
                   + NODE_UNIT_FILE + ')/dytallix-operator-rpc" --home /var/lib/dytallix/node status')
RELEASE_BIN = re.compile(r'/opt/dytallix/([A-Za-z0-9._-]+)/bin/')
DRT = ('genesis', 'emitted', 'burned', 'total', 'liquid', 'withheld_fees')
DGT = ('issued', 'liquid', 'staked', 'pending_bonded', 'unbonding', 'penalty_reserve', 'governance_escrow')
# The status view's DGT buckets the node leaves out (supply.rs,
# DgtSupplyResponse): penalty_reserve when "0", governance_escrow before
# governance accounting. The metrics file always writes every bucket.
DGT_OMITTED = ('penalty_reserve', 'governance_escrow')
PASS, FAIL, INCONCLUSIVE = 'PASS', 'FAIL', 'INCONCLUSIVE'


class Failed(Exception):
    pass


class Missing(Failed):
    """A reading without a field its schema requires: never a pass."""


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


# Readings

def metrics_supply(text):
    """The application's height and supply buckets from its metrics file
    (app_metrics.rs: `dytallix_app_height`, `dytallix_app_supply_udrt` and
    `_udgt` by `bucket`, every bucket written by each commit); None when the
    process has not committed since it started, which writes the height and
    no supply buckets."""
    samples = monitor.parse_metrics(text)
    height = monitor.value(samples, 'dytallix_app_height')
    supply = {'udrt': {}, 'udgt': {}}
    for (name, labels), amount in samples.items():
        for denomination in supply:
            if name == f'dytallix_app_supply_{denomination}':
                supply[denomination][dict(labels).get('bucket', '')] = str(amount)
    if not any(supply.values()):
        return None
    missing = ([] if height is not None else ['dytallix_app_height'])
    missing += [f'udrt {b}' for b in DRT if b not in supply['udrt']]
    missing += [f'udgt {b}' for b in DGT if b not in supply['udgt']]
    if missing:
        raise Missing(f'the metrics file has no {", ".join(missing)}')
    return int(height), supply


def field(record, name, where):
    if not isinstance(record, dict) or name not in record or record[name] is None:
        raise Missing(f'{where} has no {name}')
    return record[name]


def view_supply(view):
    """The same buckets from a status view (consensus_settlement.rs
    query_validated: `supply`, a SupplyResponse, and `dgt_supply`, a
    DgtSupplyResponse, whose omitted buckets are 0)."""
    drt, dgt = field(view, 'supply', 'the status view'), field(view, 'dgt_supply', 'the status view')
    udrt = {b: str(int(field(drt, b, 'the status view\'s supply'))) for b in DRT}
    udrt.update({f'pool_{k}': str(int(v)) for k, v in field(drt, 'pools', 'the status view\'s supply').items()})
    udgt = {b: str(int(dgt.get(b) or 0)) if b in DGT_OMITTED else str(int(field(dgt, b, 'the status view\'s dgt_supply')))
            for b in DGT}
    return {'udrt': udrt, 'udgt': udgt}


def engine_status(raw):
    """(height, block hash, header application hash) from the operator status.
    CometBFT writes the hashes in upper-case hex; the application's hash is
    lower-case hex of the same 32 bytes (the bridge's parseHash)."""
    try:
        info = json.loads(raw)['result']['sync_info']
        return int(info['latest_block_height']), info['latest_block_hash'].lower(), info['latest_app_hash'].lower()
    except (KeyError, TypeError, ValueError, AttributeError) as error:
        raise Missing(f'the operator status has no sync_info height and hashes ({error!r})') from None


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
    """The newest height read from every node's engine, which is read every
    block. The client's views are compared wherever their heights meet the
    nodes'; a CLI call takes about a block, so they are not required to."""
    sets = [set(host['engine']) for host in observations['hosts'].values()]
    common = set.intersection(*sets) if sets else set()
    return max(map(int, common)) if common else None


def attested_supply(observations):
    """Each node's newest supply reading at a height whose application hash
    every node's header recorded, all the same: (height, supply) or None.

    The application writes its metrics file on a timer (consensus_stdio.rs,
    service.metrics.interval_seconds, 15 s approved), not at each commit, and
    each node's timer runs on its own phase, so two nodes' supply readings
    need never share a height. Supply is consensus state under the
    application hash, so a node's supply at a height every node attests is
    the network's supply there."""
    hosts = observations['hosts']
    found = {}
    for label, host in hosts.items():
        found[label] = None
        for h in sorted(map(int, host['metrics']), reverse=True):
            headers = [other['engine'].get(str(h + 1), {}).get('header_app_hash') for other in hosts.values()]
            if all(headers) and len(set(headers)) == 1:
                found[label] = (h, host['metrics'][str(h)])
                break
    return found


def newest_attested(observations):
    readings = [r for r in attested_supply(observations).values() if r]
    return max(readings, key=lambda r: r[0]) if readings else None


def evaluate(observations, genesis):
    checks = Checks()
    hosts = observations['hosts']
    unreadable = observations.get('unreadable', [])
    checks.add('readings', INCONCLUSIVE if unreadable else PASS,
               '; '.join(unreadable) if unreadable else 'every reading had its schema\'s fields')
    height = common_height(observations)
    above = observations.get('common_above')
    if height is None:
        checks.add('common_height', INCONCLUSIVE, 'no height was read from every node'
                   + (f' ({"; ".join(unreadable)})' if unreadable else ''))
    elif above is not None and height <= above:
        checks.add('common_height', INCONCLUSIVE, f'every node was read together only at {height}, '
                   f'not after the state check at {above}')
    else:
        checks.add('common_height', PASS, f'height {height} read from {", ".join(sorted(hosts))}')

    # Every node's supply at a height every node's application hash attests.
    attested = attested_supply(observations)
    missing = [label for label, r in attested.items() if r is None]
    early = [f'{label} at {r[0]}' for label, r in attested.items() if r and above is not None and r[0] <= above]
    if missing or early:
        checks.add('supply_attested', INCONCLUSIVE,
                   (f'no supply reading at a height every node attests: {", ".join(missing)}' if missing else '')
                   + ('; ' if missing and early else '')
                   + (f'not after the state check at {above}: {", ".join(early)}' if early else ''))
    else:
        checks.add('supply_attested', PASS, '; '.join(
            f'{label} at {r[0]}' for label, r in sorted(attested.items())) + ', each under every node\'s application hash')

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
    newest = newest_attested(observations)
    if since and newest:
        series.append(('since the earlier reading', [(int(since['height']), since['supply']), newest]))
    for source, points in series:
        for (h0, s0), (h1, s1) in zip(points, points[1:]):
            for bucket in ('emitted', 'burned'):
                if int(s1['udrt'][bucket]) < int(s0['udrt'][bucket]):
                    regress.append(f'{source}: {bucket} fell from {s0["udrt"][bucket]} at {h0} to '
                                   f'{s1["udrt"][bucket]} at {h1}')
    if since and newest and newest[0] <= int(since['height']):
        regress.append(f'the newest attested supply at {newest[0]} is not after the earlier reading at '
                       f'{since["height"]}')
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
    newest = newest_attested(observations)
    if not since or newest is None:
        checks.add('fees_and_burns', INCONCLUSIVE, 'a ledger needs the earlier reading and an attested supply')
        return
    if ledger.get('schema') != LEDGER_SCHEMA or int(ledger['from_height']) != int(since['height']):
        checks.add('fees_and_burns', FAIL, f'the ledger does not start at the earlier reading ({since["height"]})')
        return
    at, now = newest
    burned = int(now['udrt']['burned']) - int(since['supply']['udrt']['burned'])
    charged = sum(int(e['charge_udrt']) for e in ledger['entries'])
    created = sum(int(e.get('created_accounts', 0)) for e in ledger['entries'])
    expected = charged + created * int(ledger['account_creation_fee_udrt'])
    checks.add('fees_and_burns', PASS if burned == expected else FAIL,
               f'{burned} udrt burned from {since["height"]} to {at}; {len(ledger["entries"])} transactions '
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
    if not isinstance(result.get('height'), int) or not isinstance(result.get('app_hash'), str):
        checks.add('state_check', INCONCLUSIVE, f'{recorded["host"]} passed without a height and application hash')
        return
    height, value = result['height'], result['app_hash'].lower()
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
    newest = newest_attested(observations)
    if newest is not None:
        body['supply_height'], body['supply'] = newest
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


BALANCE = ('address', 'height', 'account_exists', 'udrt', 'udgt', 'app_hash')


def reading(view):
    """A status view's height, application hash and supply buckets."""
    return {'height': int(field(view, 'height', 'the status view')),
            'app_hash': str(field(view, 'app_hash', 'the status view')), 'supply': view_supply(view)}


def balance_reading(balance):
    """`dytallix balance`'s fields (balance.rs) that the accounts check reads."""
    return {k: field(balance, k, 'a balance') for k in BALANCE}


class Collector:
    # The client's status view is read at most this often, so that the
    # nodes, read in well under a block, are read every round.
    CLIENT_EVERY = 10

    def __init__(self, hosts, client=None, clock=time.monotonic, sleep=time.sleep, say=print):
        self.hosts, self.client, self.clock, self.sleep, self.say = hosts, client, clock, sleep, say
        self.observations = {'hosts': {label: {'engine': {}, 'metrics': {}} for label in hosts},
                             'views': {}, 'client': client is not None, 'unreadable': []}
        self.client_read = None

    def unreadable(self, source, error):
        """A reading without its schema's fields: recorded once, and the result
        can then be at best INCONCLUSIVE."""
        message = f'{source}: {error}'
        if message not in self.observations['unreadable']:
            self.observations['unreadable'].append(message)
            self.say(f'== unreadable: {message}')

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
            except Missing as error:
                self.unreadable(f'{label} engine', error)
            except Exception as error:  # noqa: BLE001 - any transport's failure
                self.say(f'{label}: no engine reading ({error})')
            try:
                found = metrics_supply(host.read(METRICS).decode('utf-8', errors='replace'))
            except Missing as error:
                self.unreadable(f'{label} metrics', error)
                found = None
            except Exception:  # noqa: BLE001 - a missing file, any transport's failure
                found = None
            if found:
                record['metrics'][str(found[0])] = found[1]
        now = self.clock()
        if self.client and labels is None and (self.client_read is None or now - self.client_read >= self.CLIENT_EVERY):
            self.client_read = now
            try:
                view = reading(self.client.status())
            except Missing as error:
                self.unreadable('client', error)
                return
            except (Failed, ValueError) as error:
                self.say(f'client: no status view ({error})')
                return
            self.observations['views'][str(view['height'])] = {'app_hash': view['app_hash'], 'supply': view['supply']}

    def until_common(self, timeout, period=0.5, above=None):
        """Read until every node's engine has been read at one height and every
        node has a supply reading under an application hash every node
        attests (each above `above`). A node writes its metrics file every
        metrics interval, so this takes up to about two intervals."""
        deadline = self.clock() + timeout
        while True:
            self.sample()
            height = common_height(self.observations)
            attested = attested_supply(self.observations).values()
            if (height is not None and (above is None or height > above)
                    and all(r is not None and (above is None or r[0] > above) for r in attested)):
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
            try:
                before = reading(self.client.status())
                balances = [balance_reading(self.client.balance(address)) for address in addresses]
                after = reading(self.client.status())
            except Missing as error:
                self.unreadable('client accounts', error)
                return False
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
            state_check_host=None, scratch='/var/tmp/dytallix-reconcile-appdb', timeout=180, say=print,
            clock=time.monotonic, sleep=time.sleep):
    """Read every node, then evaluate: the evidence, whatever its status."""
    started = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    collector = Collector(hosts, client, clock=clock, sleep=sleep, say=say)
    collector.until_common(timeout)
    if client and addresses:
        collector.accounts(addresses)
    if state_check_host:
        result = collector.state_check(state_check_host, scratch)
        # The report's height comes after the check: the node is back and
        # every node agrees again.
        checked = result.get('height') if isinstance(result.get('height'), int) else None
        collector.observations['common_above'] = checked
        collector.until_common(timeout, above=checked)
    return finish(chain_id, collector, genesis_bytes, started, since, ledger)


def finish(chain_id, collector, genesis_bytes, started, since=None, ledger=None):
    observations = collector.observations
    if since is not None:
        try:
            observations['since'] = since_reading(since)
        except Missing as error:
            collector.unreadable('the earlier reading', error)
    if ledger is not None:
        observations['ledger'] = ledger
    finished = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    return evidence(chain_id, observations, genesis_bytes, started, finished)


def since_reading(record):
    """The earlier reading a ledger starts from: an earlier reconciliation or a
    saved status view."""
    if record.get('schema') == SCHEMA:
        return {'height': int(field(record, 'supply_height', 'the earlier reconciliation')),
                'supply': field(record, 'supply', 'the earlier reconciliation')}
    view = reading(record)
    return {'height': view['height'], 'supply': view['supply']}


# Through the staging agent (T2)

READ_COMMAND = 'reconcile-read'
# reconcile-read (staging-agent/ALLOWLIST.json) reads the operator status and
# the metrics file together 45 times about a second apart: about 50 s, three
# metrics writes, every block.
BURST_SECONDS = 50
# The seconds of the minute in which the founder sends, so that every host's
# next poll, on the minute by its synchronized clock, finds the command.
SEND_WINDOW = (5, 40)


def iso(seconds):
    return datetime.fromtimestamp(seconds, tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')


def burst_readings(collector, label, result):
    """One host's reconcile-read result added to the readings: each operator
    status that answered is an engine reading and each metrics file a supply
    reading, exactly as the collector reads a host directly."""
    record = collector.observations['hosts'].setdefault(label, {'engine': {}, 'metrics': {}})
    if result.get('status') not in ('DONE', 'FAILED'):
        collector.unreadable(f'{label} agent', f'{READ_COMMAND} is {result.get("status")}: {result.get("reason")}')
        return
    for step in result.get('steps') or []:
        argv, out = step.get('argv') or [], step.get('stdout') or ''
        if step.get('exit') != 0:
            continue  # a node stopped or starting gives no reading
        if out.endswith('\n[truncated]'):
            collector.unreadable(f'{label} {"engine" if argv[-1:] == ["status"] else "metrics"} (agent)',
                                 f'the agent kept only the first {64 << 10} bytes')
            continue
        if argv[-1:] == ['status']:
            try:
                height, block, app = engine_status(out)
                record['engine'][str(height)] = {'block_hash': block, 'header_app_hash': app}
            except Missing as error:
                collector.unreadable(f'{label} engine (agent)', error)
        elif argv == ['cat', METRICS]:
            try:
                found = metrics_supply(out)
            except Missing as error:
                collector.unreadable(f'{label} metrics (agent)', error)
                continue
            if found:
                record['metrics'][str(found[0])] = found[1]


def collect_through_agent(chain_id, channels, genesis_bytes, live=None, client=None, addresses=(), since=None,
                          ledger=None, align=True, after_send=None, timeout=600, say=print,
                          clock=time.monotonic, sleep=time.sleep, wall=time.time):
    """The reconciliation on console-only hosts (P01, 14 and 20 October 2026):
    reconcile-read sent through each host's agent channel (anything with
    `send(command) -> sequence` and `wait(sequences, timeout) -> results`)
    for the same minute; the client's status view, and any `live` host (the
    CI validator), read meanwhile as `collect` reads them; then each host's
    sealed result opened and its readings evaluated as if read directly. The
    evidence binds each command and result by digest. It has no state check:
    no agent command can stop a node to copy its database, so that stays at
    the console."""
    live = dict(live or {})
    if set(live) & set(channels):
        raise Failed(f'read both live and through the agent: {sorted(set(live) & set(channels))}')
    started = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    if align:
        second = wall() % 60
        if not SEND_WINDOW[0] <= second <= SEND_WINDOW[1]:
            pause = (SEND_WINDOW[0] - second) % 60
            say(f'== waiting {pause:.0f} s to send inside seconds {SEND_WINDOW[0]} to {SEND_WINDOW[1]} of the minute')
            sleep(pause)
    sent, minute = {}, wall() // 60
    for label, channel in channels.items():
        sequence = channel.send(READ_COMMAND)
        sent[label] = {'sequence': sequence, 'sent_at': iso(wall())}
    if align and (wall() // 60 != minute or wall() % 60 > 58):
        say('== the commands were sent across a minute: the hosts may read a minute apart (INCONCLUSIVE, never a pass)')
    if after_send:
        after_send()
    # The hosts read from their next poll (the next minute) for BURST_SECONDS.
    now = wall()
    until = (now - now % 60 + 60 if align else now) + BURST_SECONDS + 5
    collector = Collector(live, client, clock=clock, sleep=sleep, say=say)
    for label in channels:
        collector.observations['hosts'][label] = {'engine': {}, 'metrics': {}}
    while wall() < until:
        collector.sample()
        sleep(0.5)
    bound = {}
    for label, channel in channels.items():
        sequence = sent[label]['sequence']
        result = channel.wait([sequence], timeout)[0]
        burst_readings(collector, label, result)
        bound[label] = {**sent[label], 'command': result.get('command'), 'status': result.get('status'),
                        'command_sha256': result.get('command_sha256'), 'received_at': result.get('received_at'),
                        'finished_at': result.get('finished_at'),
                        'result': (getattr(channel, 'names', None) or {}).get(sequence)}
        say(f'== {label}: {READ_COMMAND} {result.get("status")}, '
            f'{len(collector.observations["hosts"][label]["engine"])} heights read')
    collector.observations['agent'] = bound
    if client and addresses:
        collector.accounts(addresses)
    return finish(chain_id, collector, genesis_bytes, started, since, ledger)


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
