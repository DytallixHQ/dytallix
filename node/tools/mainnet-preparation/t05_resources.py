"""T05 memory and CPU per rate step, from the hosts' monitor history
(P01_T06_T05_RESOURCE_READING_2026-11-13).

  t05_resources.py --evidence RUN/evidence.json --history HOST=FILE [--history HOST=FILE ...] [--out FILE]
  t05_resources.py --evidence RUN/evidence.json --store STORE --chain CHAIN --host HOST [--host HOST ...]
                   --paper PAPER --signer SIGNER --work DIR [--out FILE]
  (both: [--target-per-block 180] [--hold-seconds 1800])

Each host's monitor appends one line a minute to its day's history ('t', 'cpu', 'mem', ...). Once
the day is over, the monitor seals it with the chain's history code and uploads it as
RESULTS_PREFIX CHAIN/history/HOST/DAY-SHA256.bin. With --store, the reader downloads every UTC day
the run touches, checks each copy against its name's digest, and opens it with
`dytallix-root-sign history-open` and the history code paper. With --history, it reads days
already opened (.jsonl, or .jsonl.gz).

For each host and each rate step, it reports the samples and the peak and median of memory
percent and CPU percent. The steps are the intervals t05_steps.py judges (t05_steps.windows), and
the same is reported over the hold. The figures are host-wide; the hosts run nothing besides the
node and its monitor. A step a host has no sample for is reported with no figures and named under
'gaps': a hole in the history is a finding, not a zero."""
import argparse
import gzip
import hashlib
import json
import re
import statistics
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

import t05_steps

SCHEMA = 'dytallix.t05-resources.v1'
DAY_NAME = re.compile(r'^([0-9]{4}-[0-9]{2}-[0-9]{2})-([0-9a-f]{64})\.bin$')
FIELDS = (('mem', 'memory_percent'), ('cpu', 'cpu_percent'))


class Failed(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Failed(message)


def parse_lines(raw):
    """A day's history: gzip or plain JSON lines, as the monitor writes them."""
    if raw[:2] == b'\x1f\x8b':
        raw = gzip.decompress(raw)
    out = []
    for n, line in enumerate(raw.decode().splitlines(), 1):
        if line.strip():
            sample = json.loads(line)
            require(isinstance(sample, dict) and isinstance(sample.get('t'), (int, float)),
                    f'history line {n} has no time')
            out.append(sample)
    return out


def merge(samples):
    """One sample per minute stamp, in time order (a day uploaded twice reads once)."""
    return [s for _, s in sorted({s['t']: s for s in samples}.items())]


def figures(samples, lo, hi):
    inside = [s for s in samples if lo <= s['t'] < hi]
    row = {'from': stamp(lo), 'to': stamp(hi), 'samples': len(inside)}
    for key, name in FIELDS:
        values = [s[key] for s in inside if isinstance(s.get(key), (int, float))]
        row[name] = {'peak': max(values), 'median': round(statistics.median(values), 1)} if values else None
    return row


def stamp(t):
    return datetime.fromtimestamp(t, timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def days(lo, hi):
    """The UTC days [lo, hi) touches."""
    first = datetime.fromtimestamp(lo, timezone.utc).date()
    last = datetime.fromtimestamp(hi - 1e-6, timezone.utc).date()
    return [first.fromordinal(d).isoformat() for d in range(first.toordinal(), last.toordinal() + 1)]


def read(evidence, histories, target_per_block=180, hold_seconds=1800.0):
    """histories: {host: [sample, ...]}. The per-step and hold figures of each host."""
    steps, (hold_rate, hold_lo, hold_hi) = t05_steps.windows(evidence, target_per_block, hold_seconds)
    hosts, gaps = {}, []
    for host in sorted(histories):
        samples = merge(histories[host])
        rows = []
        for rate, lo, hi in steps:
            row = figures(samples, lo, hi)
            row.update(asked_per_s=rate, asked_per_block=round(rate * t05_steps.BLOCK_SECONDS, 1))
            rows.append(row)
            if not row['samples']:
                gaps.append(f'{host}: no sample in the {row["asked_per_block"]:g}/block step from {row["from"]}')
        hold = figures(samples, hold_lo, hold_hi)
        hold.update(asked_per_s=hold_rate)
        if not hold['samples']:
            gaps.append(f'{host}: no sample in the hold')
        hosts[host] = {'steps': rows, 'hold': hold}
    return {'schema': SCHEMA, 'chain_id': evidence.get('chain_id'), 'evidence_sha256': evidence.get('sha256'),
            'target_per_block': target_per_block, 'hold_seconds': hold_seconds,
            'reading': 'host-wide memory and CPU percent from the monitor history, one sample a minute',
            'hosts': hosts, 'gaps': gaps}


def fetch(store, chain, host, wanted, signer, paper, work):
    """Download, check and open the host's sealed days in `wanted`; returns their samples."""
    prefix = f'{store.store["results_prefix"]}{chain}/history/{host}/'
    found = {}
    for key in store.list(prefix):
        name = DAY_NAME.match(key[len(prefix):])
        if name and name.group(1) in wanted:
            found.setdefault(name.group(1), []).append((key, name.group(2)))
    missing = sorted(set(wanted) - set(found))
    require(not missing, f'{host}: no uploaded history for {", ".join(missing)} (a day uploads once it is over)')
    samples = []
    work.mkdir(parents=True, exist_ok=True)
    for day in sorted(found):
        for key, digest in found[day]:
            status, raw = store.get(key)
            require(status == 200 and raw is not None, f'GET {key} answered HTTP {status}')
            require(hashlib.sha256(raw).hexdigest() == digest, f'{key}: the copy does not match its name')
            sealed, opened = work / f'{host}-{day}-{digest[:12]}.bin', work / f'{host}-{day}-{digest[:12]}.jsonl.gz'
            sealed.write_bytes(raw)
            opened.unlink(missing_ok=True)
            done = subprocess.run([str(signer), 'history-open', '-paper', str(paper), '-in', str(sealed),
                                   '-out', str(opened)], capture_output=True, text=True, timeout=300)
            require(done.returncode == 0, f'history-open {key}: {done.stderr.strip()}')
            samples.extend(parse_lines(opened.read_bytes()))
    return samples


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument('--evidence', type=Path, required=True)
    p.add_argument('--history', action='append', default=[], metavar='HOST=FILE')
    p.add_argument('--store', type=Path)
    p.add_argument('--chain')
    p.add_argument('--host', action='append', default=[])
    p.add_argument('--paper', type=Path)
    p.add_argument('--signer', type=Path)
    p.add_argument('--work', type=Path)
    p.add_argument('--target-per-block', type=int, default=180)
    p.add_argument('--hold-seconds', type=float, default=1800.0)
    p.add_argument('--out', type=Path)
    a = p.parse_args(argv)
    try:
        evidence = json.loads(a.evidence.read_bytes())
        histories = {}
        if a.store:
            require(not a.history, '--history and --store are alternatives')
            require(a.chain and a.host and a.paper and a.signer and a.work,
                    '--store needs --chain, --host, --paper, --signer and --work')
            import staging_founder
            steps, _ = t05_steps.windows(evidence, a.target_per_block, a.hold_seconds)
            wanted = days(steps[0][1], steps[-1][2])
            store = staging_founder.open_store(a.store, a.work / 'store')
            for host in a.host:
                histories[host] = fetch(store, a.chain, host, wanted, a.signer, a.paper, a.work)
        else:
            require(a.history, 'name the history: --history HOST=FILE, or --store')
            for item in a.history:
                host, _, path = item.partition('=')
                require(host and path, f'--history {item}: not HOST=FILE')
                histories.setdefault(host, []).extend(parse_lines(Path(path).read_bytes()))
        result = read(evidence, histories, a.target_per_block, a.hold_seconds)
    except (Failed, t05_steps.Failed, OSError, KeyError, ValueError, subprocess.SubprocessError) as error:
        print(f't05_resources: {error}', file=sys.stderr)
        return 1
    if a.out:
        a.out.write_text(json.dumps(result, indent=2) + '\n')
    def shown(f):
        return '-' if f is None else '{}% median, {}% peak'.format(f['median'], f['peak'])
    for host, h in result['hosts'].items():
        for row in h['steps'] + [dict(h['hold'], asked_per_block='hold')]:
            print('{:12} {:>6}: {:>3} samples, memory {}; CPU {}'.format(
                host, str(row['asked_per_block']), row['samples'], shown(row['memory_percent']),
                shown(row['cpu_percent'])))
    for gap in result['gaps']:
        print(f'GAP {gap}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
