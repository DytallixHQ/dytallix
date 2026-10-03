#!/usr/bin/env python3
"""Keep LAUNCH_GATES.json's copies of decision questions equal to the register. Approves nothing.

Each gate lists its decision dependencies as copies of questions in
MAINNET_DECISION_REGISTER.json, marked with `source_ref`. The register is
authoritative: an approval is recorded there, and every copy must follow.
The gates file's `decision_counts` copies the register's `question_counts`,
which must count the register's own questions.

  decision_copies.py            report every difference; exit 1 if any
  decision_copies.py --write    copy the register's values into LAUNCH_GATES.json

`--write` changes only the copied fields and the counts. It never changes a
gate's status or the register; gate acceptance is a separate human step.
"""
import argparse
import json
from pathlib import Path
import sys

LAUNCH = Path(__file__).resolve().parents[3]/'launch'
GATES = LAUNCH/'LAUNCH_GATES.json'
REGISTER = LAUNCH/'MAINNET_DECISION_REGISTER.json'
SOURCE_REF = 'MAINNET_DECISION_REGISTER.json'
# The register fields each copy carries; `source_ref` is the copy's own marker.
COPIED = ('kind', 'status', 'question', 'blocking_output', 'assignee', 'reviewer', 'approval_record')


def render(data): return json.dumps(data, indent=2, ensure_ascii=False) + '\n'


def questions(register): return {q['id']: q for c in register['categories'] for q in c['questions']}


def copies(node, where=''):
    """(location, copy) for every embedded register question, wherever it sits."""
    if isinstance(node, dict):
        if node.get('source_ref') == SOURCE_REF: yield where, node; return
        for key, value in node.items(): yield from copies(value, f'{where}.{key}' if where else key)
    elif isinstance(node, list):
        for i, value in enumerate(node):
            label = value.get('id') if isinstance(value, dict) and 'source_ref' not in value else None
            yield from copies(value, f'{where}[{label or i}]')


def counts(register):
    """The register's question counts, recounted from its questions."""
    policy = [q['status'] for q in questions(register).values() if q['kind'] == 'policy_decision']
    return {
        'fully_open_policy_questions': policy.count('OPEN'),
        'partially_approved_policy_questions': policy.count('PARTIALLY_APPROVED'),
        'approved_policy_questions': policy.count('APPROVED'),
        'required_records': sum(q['kind'] == 'required_record' for q in questions(register).values()),
    }


def differences(gates, register):
    """Every way the gates file's copies differ from the register, one line each."""
    canonical, out = questions(register), []
    for where, copy in copies(gates):
        question = canonical.get(copy.get('id'))
        if question is None: out.append(f'{where}: {copy.get("id")!r} is not in the register'); continue
        for field in COPIED:
            if copy.get(field) != question[field]:
                out.append(f'{where}: {copy["id"]} {field} is {copy.get(field)!r}; the register has {question[field]!r}')
    if gates.get('decision_counts') != register['question_counts']:
        out.append(f'decision_counts is {gates.get("decision_counts")}; the register has {register["question_counts"]}')
    if register['question_counts'] != counts(register):
        out.append(f'the register\'s question_counts {register["question_counts"]} do not count its questions: {counts(register)}')
    return out


def sync(gates, register):
    """Copy the register's values into every copy and the counts. Gate statuses are untouched."""
    canonical = questions(register)
    for _, copy in copies(gates):
        question = canonical.get(copy.get('id'))
        if question is not None:
            for field in COPIED: copy[field] = question[field]
    gates['decision_counts'] = dict(register['question_counts'])


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--gates', type=Path, default=GATES)
    parser.add_argument('--register', type=Path, default=REGISTER)
    parser.add_argument('--write', action='store_true', help='copy the register\'s values into the gates file')
    args = parser.parse_args()
    try:
        raw = args.gates.read_text(encoding='utf-8')
        gates, register = json.loads(raw), json.loads(args.register.read_text(encoding='utf-8'))
        if args.write:
            if render(gates) != raw: raise ValueError(f'{args.gates} is not in its canonical form; refusing to rewrite it')
            sync(gates, register)
            args.gates.write_text(render(gates), encoding='utf-8')
        found = differences(gates, register)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f'INVALID: {exc}', file=sys.stderr); return 2
    for line in found: print(line, file=sys.stderr)
    print(json.dumps({'status': 'DRIFT' if found else 'IN_SYNC', 'copies': sum(1 for _ in copies(gates)), 'differences': len(found)}))
    return 1 if found else 0


if __name__ == '__main__': raise SystemExit(main())
