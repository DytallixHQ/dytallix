#!/usr/bin/env python3
"""Render and check the readable gate list from LAUNCH_GATES.json. Accepts no gate.

LAUNCH_GATES.json is the single source of gate readiness; MAINNET_GATE_MASTER.md
is generated from it. Paths under decision-register/, batch-*/, evidence/ and
snapshots/ name launch evidence records held outside this repository, so the
list shows them as text rather than links.

  gate_master.py            check the data and that the list is current; exit 1 if not
  gate_master.py --write    regenerate the list from the data

The check covers what this repository holds: gate order, statuses and counts,
complete partial records, the launch veto, and every in-repository source a gate
names (present, with valid line ranges and matching pinned digests). It never
changes a gate's status; acceptance is a separate human step.
"""
import argparse
import calendar
import hashlib
import json
from pathlib import Path
import sys

LAUNCH = Path(__file__).resolve().parents[3]/'launch'
GATES = LAUNCH/'LAUNCH_GATES.json'
MASTER = LAUNCH/'MAINNET_GATE_MASTER.md'
STATUSES = ('OPEN', 'PARTIAL', 'READY FOR ACCEPTANCE', 'PASS')
OUTSIDE = ('decision-register/', 'batch-', 'evidence/', 'snapshots/')
PACKAGE = 'decision-register/gate-consolidation-20260912'


def outside(path): return path.startswith(OUTSIDE)


def ref(path, label=None):
    """A link to a file in this repository; plain text for a record held outside it."""
    return f'`{label or path}`' if outside(path) else f'[{label or path}]({path})'


def day(iso):
    year, month, date = (int(part) for part in iso.split('-'))
    return f'{date} {calendar.month_name[month]} {year}'


def profile(p):
    """The acceptance profile's terms, as the gates apply them."""
    out = [f"**Solo launch profile** (P01, {day(p['approved'])}, [approval]({p['approval_record']}), "
           f"[trust model]({p['disclosure_ref']})). {p['rule']}", '']
    return out + [f'- **{term[0].upper() + term[1:]}:** {text}' for term, text in p['terms'].items()]


def render(d):
    out = ['# Dytallix mainnet master gate list', '',
           'Canonical data: [LAUNCH_GATES.json](LAUNCH_GATES.json). This document is generated from that file by '
           '`node/tools/mainnet-preparation/gate_master.py --write`: edit the JSON, then regenerate this view. '
           'Separate packages are evidence archives.', '',
           'Paths shown as text rather than links, under `decision-register/`, `batch-*/`, `evidence/` and `snapshots/`, '
           'name launch evidence records held outside this repository.', '',
           f"Launch: **NO GO**. Formal acceptance: **{d['passed_count']}/35**. Production records accepted: "
           f"**{d['accepted_production_records']}/8**. Final simulations completed: **{d['qualified_full_simulation_runs']}/7**.", '',
           'Current counts: ' + ', '.join(f"{d['status_counts'][k]} {k}" for k in STATUSES) + '. Partial progress can '
           'consist of preparation, implementation or local tests. It is not a percentage of completed acceptance criteria.', '']
    if d.get('acceptance_profile'): out += profile(d['acceptance_profile']) + ['']
    out += ['The gate IDs retain their original repository meanings. The conversation crosswalk ('
            + ref(PACKAGE + '/CONVERSATION_CROSSWALK.json') + ') maps the proposed work list without renumbering gates.', '',
            'Use the reconciliation report (' + ref(PACKAGE + '/RECONCILIATION_REPORT.md') + ') for source drift, superseded '
            'claims and validation limits. The evidence index (' + ref(PACKAGE + '/PRIMARY_EVIDENCE_INDEX.json') + ') '
            'preserves all indexed supporting records. Do not add overlapping test counts.', '',
            '| Gate | Requirement category | Status |', '|---|---|---|']
    out += [f"| [{g['id']}](#{g['id'].lower()}) | {g['category']} | {g['status']} |" for g in d['gates']]
    out += ['', '## Status and acceptance rules', ''] + [f'- **{k}:** {v}' for k, v in d['status_definitions'].items()]
    out += ['', d['launch_acceptance_requirement'], '']
    for g in d['gates']:
        out += [f"<a id=\"{g['id'].lower()}\"></a>", f"## {g['id']} — {g['category']}", '', f"**Status: {g['status']}**", '',
                f"**Requirement:** {g['requirement']}", '', '**Completed work**', '']
        out += [f'- {s}' for s in g['completed_work']]
        out += ['', '**Evidence**', ''] + [f"- {ref(e['path'])}" for e in g['evidence']]
        out += ['', "Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, "
                'source/artifact hashes, logs and execution limits.', '', '**Remaining work**', '']
        out += [f'- {s}' for s in g['remaining_work']]
        a = g['acceptance_requirement']
        out += ['', f"**Acceptance requirement:** {a['gate_specific']}", ''] + [f'- {s}' for s in a['common']]
        out += [f"- {s['id']}: {s['required_evidence']}" for s in a.get('runbook_requirements') or []]
        out += ['', f"**Acceptance authority:** {a['owner_role']}. Named assignee, independent reviewer and acceptance record remain unset.",
                '', f"**Current blocker:** {g['current_blocker']}", '',
                '**Decision and record dependencies:** ' + ', '.join(q['id'] for q in g['decision_dependencies'])
                + ". See exact questions and source status in this gate's JSON record.", '', '**Requirement sources**', '']
        for e in g['requirement_refs']:
            label = e['path']
            if 'line_start' in e: label += f" lines {e['line_start']}–{e['line_end']}"
            elif 'json_pointer' in e: label += ' ' + e['json_pointer']
            elif 'section' in e: label += ': ' + e['section']
            out.append('- ' + ref(e['path'], label))
        if 'formal_specification' in g:
            out += ['', '**Exact G35 required evidence**', ''] + [f'- {s}' for s in g['formal_specification']['required_evidence']]
            out += ['', '**Original formal pass rule:** ' + g['formal_specification']['pass_rule'],
                    'The archived FAIL term denotes failed formal qualification. The current progress term is PARTIAL. The launch veto remains active.']
        if g.get('current_source_note'): out += ['', '**Current source boundary:** ' + g['current_source_note']]
        out += ['']
    return '\n'.join(out).rstrip() + '\n'


def sha256(path): return hashlib.sha256(path.read_bytes()).hexdigest()


def sources(d):
    """(gate, entry) for every file a gate names: evidence and requirement sources."""
    for g in d['gates']:
        for e in g['evidence'] + g['requirement_refs']: yield g['id'], e


def problems(d, master, root=LAUNCH):
    """Every way the data or the readable list fails the check, one line each."""
    out, gates = [], d['gates']
    if [g['id'] for g in gates] != [f'G{i:02}' for i in range(1, 36)]: return ['the gates must be exactly G01 to G35, in order']
    for g in gates:
        name = g['id']
        if g['status'] not in STATUSES: out.append(f'{name}: unsupported status {g["status"]!r}')
        if not (g['requirement'] and g['requirement_refs']): out.append(f'{name}: requirement or its sources missing')
        if g['status'] == 'PARTIAL' and not all(g[k] for k in ('completed_work', 'evidence', 'remaining_work', 'current_blocker')):
            out.append(f'{name}: a partial gate needs completed work, evidence, remaining work and a blocker')
        if g['status'] in ('READY FOR ACCEPTANCE', 'PASS') and g['remaining_work']: out.append(f'{name}: {g["status"]} with remaining work')
        if g['status'] == 'PASS' and g['current_blocker']: out.append(f'{name}: PASS with a blocker')
    counts = {s: sum(g['status'] == s for g in gates) for s in STATUSES}
    if d['status_counts'] != counts: out.append(f'status_counts is {d["status_counts"]}; the gates count {counts}')
    if d['passed_count'] != counts['PASS'] or d['readiness_percent'] != 100 * counts['PASS'] / 35:
        out.append('passed_count and readiness_percent must follow the PASS gates')
    complete = counts['PASS'] == 35 and d['qualified_full_simulation_runs'] == 7 and d['accepted_production_records'] == 8
    if not complete and (d['status'] != 'NO_GO' or d['production_activation_authorized'] is not False):
        out.append('launch must stay NO_GO, without production activation, until every gate, simulation and record is complete')
    if not (gates[-1].get('mandatory') is True and gates[-1].get('launch_veto') is True): out.append('G35 must stay mandatory with its launch veto')
    for name, e in sources(d):
        if outside(e['path']): continue
        path = root/e['path']
        if not path.is_file(): out.append(f'{name}: {e["path"]} is not in this repository'); continue
        if 'line_end' in e and not 1 <= e['line_start'] <= e['line_end'] <= len(path.read_text(encoding='utf-8').splitlines()):
            out.append(f'{name}: {e["path"]} has no lines {e["line_start"]}–{e["line_end"]}')
        if 'sha256' in e and sha256(path) != e['sha256']: out.append(f'{name}: {e["path"]} differs from its pinned digest')
    if master != render(d): out.append(f'{MASTER.name} is stale; run tools/mainnet-preparation/gate_master.py --write')
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--write', action='store_true', help='regenerate the readable list from the data')
    args = parser.parse_args()
    try:
        d = json.loads(GATES.read_text(encoding='utf-8'))
        if args.write: MASTER.write_text(render(d), encoding='utf-8')
        found = problems(d, MASTER.read_text(encoding='utf-8'))
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f'INVALID: {exc}', file=sys.stderr); return 2
    for line in found: print(line, file=sys.stderr)
    print(json.dumps({'status': 'FAIL' if found else 'PASS', 'gates': len(d['gates']), 'status_counts': d['status_counts'],
                      'launch': d['status'], 'problems': len(found)}))
    return 1 if found else 0


if __name__ == '__main__': raise SystemExit(main())
