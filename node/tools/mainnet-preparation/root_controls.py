#!/usr/bin/env python3
"""The attack campaign's root-control family (simulation plan, statistical
campaign; launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json): each
type at least 3,000 times on the staging hosts, with zero successes.

  root_controls.py verify --evidence FILE --rows FILE --genesis NATIVE_GENESIS
                          --engine-genesis ENGINE_GENESIS

`staging_network.py root-control-campaign` runs it on the staging network,
after the kit replacement drill: the release's own tools and the staging
kits make the genuine controls each type starts from (the desk), and every
attempt goes to the public node through the release CLI over the client
channel. Each type and what it sends:
  - too_few_signatures: a fresh freeze signed by three freeze keys, with one
    signature left out;
  - wrong_epoch_keys: the same freeze with one signature swapped for one by
    the freeze key the kit drill replaced, signed from a stale key list as
    an old kit would;
  - non_canonical: the same freeze re-encoded (a space before it, an escaped
    letter, two fields swapped), so its value and signatures are the
    genuine ones but its bytes are not canonical;
  - expired_window: a freeze signed for a short window, sent once the chain
    is past it;
  - replayed_sequence: the kit drill's committed freeze and resume, byte for
    byte, checked (the engine's cache answers committed bytes before the
    node can), and the freeze altered as below, sent;
  - kit_replacement_while_frozen: a fresh kit replacement the node would
    admit, sent while the fresh freeze holds the chain; the chain is then
    resumed, and the replacement is never sent unfrozen.
Each type's first attempt sends the genuine control with its one fault.
Later ones change one more field by the attempt number so each has its own
bytes: a freeze's incident digest, a replacement's last height. Their
signatures then no longer verify, but every check these types name runs
before the node verifies a signature. Each row records which fields differ
from the genuine control and whether its bytes are canonical, and counts as
refused only when the node gave the type's reason and those differences are
exactly the type's.

An admitted attempt is a success (a defect). A refusal for another reason
is counted apart and fails the type (the attempt did not test what it
names). An attempt the CLI could not make is counted apart and made again.
After each type a positive control: a control the node should admit is
checked and would be admitted (the fresh freeze; while frozen, the resume;
after it, the replacement), and the chain's height rises from control to
control, so a node that refuses everything, or a chain that stopped,
cannot pass.

verify recomputes every outcome, count, status and digest from the rows; the
evidence binds the genesis files and names each genuine control by digest.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import attacks  # noqa: E402

SCHEMA = attacks.SCHEMA
FAMILY = 'root_controls'
PASS, FAIL = attacks.PASS, attacks.FAIL
TARGET = 3000
OUTCOMES = attacks.OUTCOMES
EMERGENCY_KIND = 'dytallix-emergency-control-v2'
# The node's words for each type (consensus_settlement.rs, emergency_freeze.rs).
# A control that is not its canonical bytes is no root control at all, so the
# wire decoder's last try, the tagged transaction, names its kind.
REASONS = {
    'too_few_signatures': 'Emergency signature threshold',
    'wrong_epoch_keys': 'Emergency authority key missing',
    'non_canonical': f'unknown variant `{EMERGENCY_KIND}`',
    'expired_window': 'Emergency v2 outside validity window',
    'replayed_sequence': 'Emergency sequence mismatch/exhaustion',
    'kit_replacement_while_frozen': 'Emergency transaction freeze',
}
INCIDENT = 'payload.v2.incident_sha256'
LAST_HEIGHT = 'payload.not_after_height'
# The fields each attempt changes from its genuine control, by variant.
CHANGES = {
    'too_few_signatures': {'signed': {'signatures'}, 'altered': {'signatures', INCIDENT}},
    'wrong_epoch_keys': {'signed': {'signatures'}, 'altered': {'signatures', INCIDENT}},
    'non_canonical': {'space': set(), 'escape': set(), 'reorder': set()},
    'expired_window': {'signed': set(), 'altered': {INCIDENT}},
    'replayed_sequence': {'freeze': set(), 'resume': set(), 'altered': {INCIDENT}},
    'kit_replacement_while_frozen': {'signed': set(), 'altered': {LAST_HEIGHT}},
}
# Committed bytes go to CheckTx directly: the engine's cache would answer a
# broadcast of them first.
CHECKED = {('replayed_sequence', 'freeze'), ('replayed_sequence', 'resume')}
TRIES = 3
CONTROL_TIMEOUT = 120


class Failed(Exception):
    pass


class Unavailable(Exception):
    """The CLI could not make the attempt (no answer from the node)."""


digest, rows_bytes = attacks.digest, attacks.rows_bytes


def record_digest(value):
    return digest(attacks.canonical(value))


# The bytes each attempt sends

def encode(value):
    """A control's canonical bytes: its fields in the node's order, compact."""
    return json.dumps(value, separators=(',', ':')).encode()


def in_order_of(model, value):
    """`value` with its fields in `model`'s order, as the node writes them."""
    if isinstance(model, dict) and isinstance(value, dict):
        keys = [k for k in model if k in value] + [k for k in value if k not in model]
        return {k: in_order_of(model.get(k), value[k]) for k in keys}
    return value


def is_canonical(raw, genuine=None):
    """Whether bytes are the node's encoding of their value: compact, and
    with the fields in the genuine control's order (the node's own)."""
    try:
        value = json.loads(raw)
        return encode(in_order_of(json.loads(genuine or raw), value)) == raw
    except ValueError:
        return False


def changed(a, b, path=''):
    """The dotted paths at which two JSON values differ; a list is one field."""
    if isinstance(a, dict) and isinstance(b, dict):
        found = set()
        for key in a.keys() | b.keys():
            found |= changed(a.get(key), b.get(key), f'{path}.{key}' if path else key)
        return found
    return set() if a == b else {path}


def altered(value, kind, k):
    """A copy with the one field an attempt changes by its number."""
    value = copy.deepcopy(value)
    if kind == 'kit_replacement_while_frozen':
        payload = value['payload']
        if k >= payload['not_after_height'] - payload['not_before_height']:
            raise Failed('the replacement\'s window is shorter than the attempts')
        payload['not_after_height'] -= k
    else:
        value['payload']['v2']['incident_sha256'] = digest(f'{kind} {k}'.encode())
    return value


def reencoded(raw, k):
    """The genuine control's value in bytes that are not canonical: a space
    before it, the first key's letter escaped, or its first two fields
    swapped; after the first round, k // 3 spaces after it as well."""
    variant = ('space', 'escape', 'reorder')[k % 3]
    value = json.loads(raw)
    if variant == 'space':
        text = b' ' + raw
    elif variant == 'escape':
        if not raw.startswith(b'{"kind":'):
            raise Failed('the control does not start with its kind')
        text = b'{"\\u006bind":' + raw[len(b'{"kind":'):]
    else:
        first, second, *rest = value
        text = encode({second: value[second], first: value[first], **{key: value[key] for key in rest}})
    return variant, text + b' ' * (k // 3)


def variant_of(kind, k):
    if kind == 'non_canonical':
        return ('space', 'escape', 'reorder')[k % 3]
    if kind == 'replayed_sequence':
        return ('freeze', 'resume', 'altered')[k % 3]
    return 'signed' if k == 0 else 'altered'


def forge(kind, k, desk):
    """(variant, route, bytes, genuine bytes) of attempt k."""
    variant = variant_of(kind, k)
    if kind in ('too_few_signatures', 'wrong_epoch_keys'):
        genuine = desk.template
        value = json.loads(genuine)
        if kind == 'too_few_signatures':
            value['signatures'] = value['signatures'][:-1]
        else:
            kept = value['signatures'][1:]
            if any(s['key_id'] == desk.old_signature['key_id'] for s in kept):
                raise Failed('the replaced key still signs the template')
            value['signatures'] = sorted(kept + [dict(desk.old_signature)], key=lambda s: s['key_id'])
        raw = encode(value if variant == 'signed' else altered(value, kind, k))
    elif kind == 'non_canonical':
        genuine = desk.template
        variant, raw = reencoded(genuine, k)
    elif kind == 'expired_window':
        genuine = desk.expired
        raw = genuine if variant == 'signed' else encode(altered(json.loads(genuine), kind, k))
    elif kind == 'replayed_sequence':
        genuine = desk.consumed['resume' if variant == 'resume' else 'freeze']
        raw = genuine if variant != 'altered' else encode(altered(json.loads(genuine), kind, k))
    elif kind == 'kit_replacement_while_frozen':
        genuine = desk.replacement
        raw = genuine if variant == 'signed' else encode(altered(json.loads(genuine), kind, k))
    else:
        raise Failed(f'{kind!r} is not an approved root-control attack type')
    return variant, ('check' if (kind, variant) in CHECKED else 'submit'), raw, genuine


def facts(raw, genuine):
    """What a row keeps of an attempt's bytes: their digest and size, which
    fields differ from the genuine control and whether they are canonical."""
    return {'sha256': digest(raw), 'size': len(raw), 'changed': sorted(changed(json.loads(genuine), json.loads(raw))),
            'canonical': is_canonical(raw, genuine)}


def as_named(row):
    """Whether an attempt changed exactly its type's fields, in canonical
    bytes, or for non_canonical none, in bytes that are not."""
    kind, variant = row['type'], row['variant']
    wanted = CHANGES.get(kind, {}).get(variant)
    return wanted is not None and set(row['changed']) == wanted and row['canonical'] == (kind != 'non_canonical')


def classify(row):
    """An attempt's outcome from the node's answer and its own facts."""
    if row['type'] not in REASONS:
        raise Failed(f'{row["type"]!r} is not an approved root-control attack type')
    code = row.get('code')
    if not isinstance(code, int) or isinstance(code, bool):
        return 'transport'
    if code == 0:
        return 'admitted'
    if not as_named(row):
        return 'refused_other'
    return 'refused' if REASONS[row['type']] in (row.get('log') or '') else 'refused_other'


def control_ok(row):
    return row.get('code') == 0 and isinstance(row.get('height'), int)


class Campaign:
    """Each type's attempts against the node, through the desk."""

    def __init__(self, desk, attempts, show=print, clock=time.monotonic, sleep=time.sleep):
        self.desk, self.attempts, self.show, self.clock, self.sleep = desk, attempts, show, clock, sleep
        self.rows, self.height = [], None

    def send(self, route, raw):
        answer = (self.desk.check if route == 'check' else self.desk.submit)(raw)
        return {'code': answer['code'], 'log': (answer.get('log') or '')[:400]}

    def attempt(self, kind, k):
        variant, route, raw, genuine = forge(kind, k, self.desk)
        row = {'type': kind, 'k': k, 'variant': variant, 'route': route, **facts(raw, genuine)}
        if not as_named(row):
            raise Failed(f'{kind} attempt {k} is not what it names: {row["changed"]}, canonical {row["canonical"]}')
        for _ in range(TRIES):
            try:
                found = {**row, **self.send(route, raw)}
            except Unavailable as error:
                found = {**row, 'error': str(error)[:200]}
            found['outcome'] = classify(found)
            self.rows.append(found)
            if found['outcome'] != 'transport':
                return found['outcome']
        raise Failed(f'{kind} attempt {k} could not be made {TRIES} times running')

    def control(self, after, what):
        """The positive control: one block more than at the last control,
        then a control the node should admit, checked."""
        deadline = self.clock() + CONTROL_TIMEOUT
        height = self.desk.height()
        while self.height is not None and height <= self.height:
            if self.clock() > deadline:
                raise Failed(f'the chain did not advance past height {self.height}')
            self.sleep(1)
            height = self.desk.height()
        raw = getattr(self.desk, what)
        try:
            answer = self.send('check', raw)
        except Unavailable as error:
            answer = {'error': str(error)[:200]}
        row = {'type': 'control', 'after': after, 'what': what, 'sha256': digest(raw), 'height': height, **answer}
        self.rows.append(row)
        self.height = height
        if not control_ok(row):
            self.show(f'== the positive control after {after} was not admitted: {answer}')

    def run_type(self, kind):
        self.show(f'== {kind}: {self.attempts} attempts')
        outcomes = [self.attempt(kind, k) for k in range(self.attempts)]
        counts = {o: outcomes.count(o) for o in OUTCOMES if o != 'transport'}
        said = next((r['log'] for r in reversed(self.rows) if r['type'] == kind and 'log' in r), None)
        self.show(f'== {kind}: {counts}; the node said {said!r}')

    def run(self, kinds):
        for kind in kinds:
            if kind not in REASONS:
                raise Failed(f'{kind!r} is not an approved root-control attack type')
        # The attempts are rebuilt from these values, so each must be exactly
        # the bytes this campaign writes for it, before anything is sent.
        for name, raw in genuine(self.desk):
            if encode(json.loads(raw)) != raw:
                raise Failed(f'the {name} control is not in the encoding this campaign writes; no attempt was made')
        self.control('start', 'template')
        frozen = [k for k in kinds if k == 'kit_replacement_while_frozen']
        for kind in (k for k in kinds if k not in frozen):
            if kind == 'expired_window':
                self.desk.wait_expired()
            self.run_type(kind)
            self.control(kind, 'template')
        if frozen and any(r.get('outcome') == 'admitted' for r in self.rows):
            # An admitted control may have changed the freeze state or the
            # sequence the freeze needs; the record already fails.
            self.show(f'== {frozen[0]}: not run, since an attack was admitted')
        elif frozen:
            self.control('replacement', 'replacement')
            self.desk.freeze()
            self.run_type(frozen[0])
            self.control(frozen[0], 'resume_control')
            self.desk.resume()
            self.control('resumed', 'replacement')
        return self.rows


def genuine(desk):
    """Each genuine control the desk made or read, by name."""
    return [('template', desk.template), ('expired', desk.expired), ('replacement', desk.replacement),
            *((f'committed {name}', raw) for name, raw in sorted(desk.consumed.items()))]


def tally(rows, attempts):
    """Each type's outcome counts, first log and status: PASS once the wanted
    attempts were refused with no success and no other refusal."""
    types = {}
    for kind in REASONS:
        mine = [r for r in rows if r['type'] == kind]
        counts = {o: sum(1 for r in mine if r['outcome'] == o) for o in OUTCOMES}
        if counts['admitted'] or counts['refused_other']:
            status = FAIL
        elif not mine:
            status = 'not_run'
        else:
            status = PASS if counts['refused'] >= attempts else FAIL
        types[kind] = {'reason': REASONS[kind], 'counts': counts, 'status': status,
                       'first_log': next((r['log'] for r in mine if 'log' in r), None)}
    return types


def build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, materials, started, finished):
    for row in rows:
        if row['type'] != 'control' and classify(row) != row['outcome']:
            raise Failed(f'a {row["type"]} row\'s outcome does not follow from its answer')
    raw = rows_bytes(rows)
    types = tally(rows, attempts)
    controls = [r for r in rows if r['type'] == 'control']
    heights = [r.get('height') for r in controls]
    checks = {
        'types': all(t['status'] in (PASS, 'not_run') for t in types.values()),
        'controls': bool(controls) and all(map(control_ok, controls)),
        'advancing': all(isinstance(h, int) for h in heights) and all(a < b for a, b in zip(heights, heights[1:])),
        # A replacement is refused while the chain is frozen, so its control
        # after the resume shows the campaign left the chain running.
        'resumed': types['kit_replacement_while_frozen']['status'] == 'not_run'
        or (bool(controls) and controls[-1]['after'] == 'resumed'),
    }
    body = {'schema': SCHEMA, 'family': FAMILY, 'chain_id': chain_id,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'started': started, 'finished': finished, 'attempts_wanted': attempts, 'materials': materials,
            'types': types,
            'controls': {'made': len(controls), 'admitted': sum(map(control_ok, controls))},
            'successes': sum(t['counts']['admitted'] for t in types.values()),
            'rows_sha256': digest(raw), 'checks': checks,
            'status': PASS if all(checks.values()) else FAIL}
    body['sha256'] = record_digest(body)
    return body


def verify(record, raw, genesis_bytes, engine_genesis_bytes):
    """Re-evaluates a campaign record from its rows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or record_digest(body) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['family'] != FAMILY:
        raise Failed(f'the record is for {record["family"]}, not {FAMILY}')
    if digest(raw) != record['rows_sha256']:
        raise Failed('the rows are not the ones the evidence names')
    if record['genesis'] != {'native_genesis_sha256': digest(genesis_bytes),
                             'engine_genesis_sha256': digest(engine_genesis_bytes)}:
        raise Failed('the evidence names another genesis')
    rows = [json.loads(line) for line in raw.splitlines() if line.strip()]
    again = build_record(record['chain_id'], genesis_bytes, engine_genesis_bytes, rows, record['attempts_wanted'],
                         record['materials'], record['started'], record['finished'])
    if again != record:
        raise Failed('the counts and statuses do not follow from the rows')
    return record['status']


def materials_of(desk):
    """The genuine controls each type started from, by digest."""
    window = json.loads(desk.expired)['payload']['v2']
    return {'template_sha256': digest(desk.template), 'expired_sha256': digest(desk.expired),
            'expired_window': [window['not_before_height'], window['not_after_height']],
            'replayed_sha256': {name: digest(raw) for name, raw in sorted(desk.consumed.items())},
            'replacement_sha256': digest(desk.replacement), 'old_key_id': desk.old_signature['key_id']}


def stamp():
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def run_campaign(desk, out, attempts, kinds, chain_id, genesis_bytes, engine_genesis_bytes, show=print):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    started = stamp()
    rows = Campaign(desk, attempts, show).run(kinds)
    record = build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, materials_of(desk),
                          started, stamp())
    (out / 'root-controls.jsonl').write_bytes(rows_bytes(rows))
    (out / 'root-controls.json').write_text(json.dumps(record, indent=2) + '\n')
    return record


def report(record, show=print):
    for kind, t in record['types'].items():
        show(f'== {kind}: {t["status"]} {t["counts"]}; first: {(t["first_log"] or "")[:160]!r}')
    show(f'== controls {record["controls"]}, successes {record["successes"]}, checks {record["checks"]}: '
         f'{record["status"]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    for name in ('--evidence', '--rows', '--genesis', '--engine-genesis'):
        v.add_argument(name, type=Path, required=True)
    args = parser.parse_args()
    try:
        status = verify(json.loads(args.evidence.read_bytes()), args.rows.read_bytes(), args.genesis.read_bytes(),
                        args.engine_genesis.read_bytes())
    except (Failed, KeyError, TypeError, ValueError) as error:
        print(f'root_controls: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    sys.exit(main())
