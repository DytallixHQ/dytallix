#!/usr/bin/env python3
"""Build and check the launch page's data, launch/simulations/dashboard.json.

The page (dytallix.com/developers/launch, schema dytallix.launch-dashboard.v1)
reads the file from main and accepts it only if it passes the page's own
checks, which this tool repeats (page_problems). Every status and value comes
from a record in this repository, as launch/simulations/
DASHBOARD_DEFINITIONS.json names it for each KPI: an approval record's date; a
tool's build date, proven by its CI step named "(Tn)" in host-install.yml; or
results, from the newest signed result summary that reports the KPI
(launch/simulations/results/, written and signed by results.py), so a result
KPI stays planned until one does. Nothing is entered by hand. as_of is the newest date among the KPIs and the
approval records, so a decision moves it too. The launch gates (G01 to G35)
are copied from launch/LAUNCH_GATES.json: each gate's ID, area, status and
current blocker, with the tracker's own status mapped to the page's (OPEN is
planned, PARTIAL and READY FOR ACCEPTANCE are in progress, PASS is pass).
Every summary
must have its signature file beside it and pass summary_problems (the page's
rules, and the KPI's unit and target); the signatures themselves are checked
with the release CLI (results.py verify), which this check does not run.

  launch_dashboard.py            check that the file is current; exit 1 if not
  launch_dashboard.py --write    regenerate it (needs the full history: it counts commits)

The output is deterministic: no clock, and no commit of its own. The check
ignores one field, project.commits_at_as_of (the commits on main up to
as_of), which no committed file can keep current.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
DEFINITIONS = 'launch/simulations/DASHBOARD_DEFINITIONS.json'
DASHBOARD = 'launch/simulations/dashboard.json'
RESULTS = 'launch/simulations/results'
RESULT_SCHEMA = 'dytallix.launch-result.v1'
# An ML-DSA-65 signature (3,309 bytes) in hex.
SIGNATURE = re.compile(r'[0-9a-f]{6618}')
WORKFLOW = '.github/workflows/host-install.yml'
GATES = 'launch/LAUNCH_GATES.json'
GATES_PAGE = 'launch/MAINNET_GATE_MASTER.md'
GATE_STATUS = {'OPEN': 'planned', 'PARTIAL': 'in_progress', 'READY FOR ACCEPTANCE': 'in_progress', 'PASS': 'pass'}
GATE_ID = re.compile(r'G[0-9]{2}')
APPROVALS = 'launch/approvals'
DEFINITIONS_SCHEMA = 'dytallix.launch-dashboard-definitions.v1'
REPO = 'https://github.com/DytallixHQ/dytallix'
# The site's own copy is from 11 October 2026; the page keeps it over an older file.
SITE_COPY_AS_OF = '2026-10-11'
# Launch server names, which the page rules forbid as well as addresses.
HOST_NAMES = ('hel1', 'fsn1', 'nbg1', 'ash-1', 'ubuntu-4gb', 'docker-ce')

# The page's checks, as dytallix-site src/launch/dashboardData.js has them.
SCHEMA = 'dytallix.launch-dashboard.v1'
STATUSES = ('planned', 'in_progress', 'pass', 'fail', 'blocked', 'not_applicable')
UNITS = (None, 's', 'count', '%', 'days')
DATE = re.compile(r'\d{4}-\d{2}-\d{2}', re.ASCII)
KPI_ID = re.compile(r'[a-z]+\.[a-z0-9_]+\.[a-z0-9_]+')
PHASE_ID = re.compile(r'[a-z_]+')
GROUP_ID = re.compile(r'[A-Za-z0-9_+-]{1,32}')
IPV4 = re.compile(r'\b(?:\d{1,3}\.){3}\d{1,3}\b', re.ASCII)
MAX_TEXT = 2000
MAX_SAFE = 2 ** 53 - 1
MONTHS = ('January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October',
          'November', 'December')


class Invalid(ValueError):
    pass


def sha256(raw): return hashlib.sha256(raw).hexdigest()


def is_date(value): return isinstance(value, str) and DATE.fullmatch(value) is not None


def long_date(date):
    year, month, day = (int(part) for part in date.split('-'))
    return f'{day} {MONTHS[month - 1]} {year}'


def number(value): return isinstance(value, (int, float)) and not isinstance(value, bool)


def safe_count(value): return isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= MAX_SAFE


# The page's address checks: times such as 14:00:00 and code paths such as
# fee::adapt are not addresses.
def has_ipv6(value):
    for token in re.findall(r'[0-9a-f:]{6,}', value, re.IGNORECASE):
        parts = token.split(':')
        if len(parts) < 3 or len(parts) > 8 or not re.search(r'\d', token) or any(len(p) > 4 for p in parts):
            continue
        if '::' in token or len(parts) == 8:
            return True
    return False


def contains_address(value): return IPV4.search(value) is not None or has_ipv6(value)


def page_problems(data):
    """validateDashboard: the checks the page runs before it shows a file."""
    errors = []

    def text(value, path):
        if not isinstance(value, str) or not value.strip(): errors.append(f'{path} must be text')
        elif len(value.encode('utf-16-le')) // 2 > MAX_TEXT: errors.append(f'{path} is too long')
        elif contains_address(value): errors.append(f'{path} contains an address')

    def evidence(value, path):
        if value is None: return
        if not isinstance(value, str) or not (value == REPO or value.startswith(f'{REPO}/')) or re.search(r'\s', value):
            errors.append(f'{path} must link to {REPO}')

    def items(value, path, most):
        if not isinstance(value, list) or len(value) > most:
            errors.append(f'{path} must be a list of at most {most}')
            return []
        return value

    if not isinstance(data, dict): return ['not an object']
    if data.get('schema') != SCHEMA: errors.append('unknown schema')
    if not is_date(data.get('as_of')): errors.append('as_of must be a date')
    if not isinstance(data.get('source'), dict) or data['source'].get('repo') != REPO:
        errors.append('unknown source repository')
    for index, rule in enumerate(items(data.get('schema_rules'), 'schema_rules', 20)):
        text(rule, f'schema_rules[{index}]')
    if 'project' in data:
        project = data['project']
        if not isinstance(project, dict) or not is_date(project.get('since')) or not safe_count(
                project.get('commits_at_as_of')) or not safe_count(project.get('approval_records')):
            errors.append('project is invalid')
    phases = set()
    for index, phase in enumerate(items(data.get('phases'), 'phases', 20)):
        path, phase = f'phases[{index}]', phase if isinstance(phase, dict) else {}
        if not isinstance(phase.get('id'), str) or not PHASE_ID.fullmatch(phase['id']) or phase['id'] in phases:
            errors.append(f'{path}.id is invalid')
        else: phases.add(phase['id'])
        if not isinstance(phase.get('order'), int) or isinstance(phase.get('order'), bool):
            errors.append(f'{path}.order must be a whole number')
        text(phase.get('name'), f'{path}.name')
        text(phase.get('summary'), f'{path}.summary')
        if phase.get('status') not in STATUSES: errors.append(f'{path}.status is invalid')
        evidence(phase.get('evidence_url'), f'{path}.evidence_url')
    groups = set()
    for index, group in enumerate(items(data.get('groups'), 'groups', 200)):
        path, group = f'groups[{index}]', group if isinstance(group, dict) else {}
        key = f'{group.get("phase")}/{group.get("id")}'
        if not isinstance(group.get('id'), str) or not GROUP_ID.fullmatch(group['id']) or key in groups:
            errors.append(f'{path}.id is invalid')
        else: groups.add(key)
        if group.get('phase') not in phases: errors.append(f'{path}.phase is unknown')
        text(group.get('name'), f'{path}.name')
        text(group.get('summary'), f'{path}.summary')
    ids = set()
    for index, kpi in enumerate(items(data.get('kpis'), 'kpis', 1000)):
        path, kpi = f'kpis[{index}]', kpi if isinstance(kpi, dict) else {}
        if not isinstance(kpi.get('id'), str) or not KPI_ID.fullmatch(kpi['id']) or kpi['id'] in ids:
            errors.append(f'{path}.id is invalid')
        else: ids.add(kpi['id'])
        if kpi.get('phase') not in phases: errors.append(f'{path}.phase is unknown')
        if f'{kpi.get("phase")}/{kpi.get("group")}' not in groups: errors.append(f'{path}.group is unknown')
        for field in ('name', 'measures', 'why', 'threshold'): text(kpi.get(field), f'{path}.{field}')
        if kpi.get('unit') not in UNITS or 'unit' not in kpi: errors.append(f'{path}.unit is invalid')
        if kpi.get('status') not in STATUSES: errors.append(f'{path}.status is invalid')
        # A missing value is undefined on the page, which is not null.
        value = kpi.get('value', '')
        if number(value):
            if not math.isfinite(value): errors.append(f'{path}.value is invalid')
        elif value is not None: text(value, f'{path}.value')
        evidence(kpi.get('evidence_url'), f'{path}.evidence_url')
        target = kpi.get('target')
        if target is not None and not (number(target) and math.isfinite(target)):
            errors.append(f'{path}.target must be a number')
        if not is_date(kpi.get('updated')): errors.append(f'{path}.updated must be a date')
    if 'gates' in data:
        gates = data['gates'] if isinstance(data['gates'], dict) else {}
        evidence(gates.get('evidence_url'), 'gates.evidence_url')
        seen = set()
        for index, gate in enumerate(items(gates.get('items'), 'gates.items', 100)):
            path, gate = f'gates.items[{index}]', gate if isinstance(gate, dict) else {}
            if not isinstance(gate.get('id'), str) or not GATE_ID.fullmatch(gate['id']) or gate['id'] in seen:
                errors.append(f'{path}.id is invalid')
            else: seen.add(gate['id'])
            text(gate.get('name'), f'{path}.name')
            if gate.get('status') not in STATUSES: errors.append(f'{path}.status is invalid')
            if gate.get('next') is not None: text(gate['next'], f'{path}.next')
    return errors


def roll_up(statuses):
    """The page's rollUp: one failure fails a group; it passes when every applicable check passes."""
    applicable = [s for s in statuses if s != 'not_applicable']
    if not applicable: return 'not_applicable'
    if 'fail' in applicable: return 'fail'
    if all(s == 'pass' for s in applicable): return 'pass'
    if 'blocked' in applicable: return 'blocked'
    if any(s != 'planned' for s in applicable): return 'in_progress'
    return 'planned'


def proven_tools(workflow):
    """The tools whose CI step, named "... (Tn)", runs in the host install workflow."""
    return set(re.findall(r'^\s*- name: .*\((T\d+)\)\s*$', workflow, re.MULTILINE))


def gather(root=ROOT):
    """Every input the dashboard is built from, read once."""
    root = Path(root)
    definitions = (root / DEFINITIONS).read_bytes()
    workflow = (root / WORKFLOW).read_bytes()
    d = json.loads(definitions)
    if d.get('schema') != DEFINITIONS_SCHEMA: raise Invalid(f'{DEFINITIONS} is not {DEFINITIONS_SCHEMA}')
    gates = (root / GATES).read_bytes()
    records, digests = {}, [(DEFINITIONS, definitions), (WORKFLOW, workflow), (GATES, gates)]
    summaries = []
    for path in sorted((root / RESULTS).rglob('*')) if (root / RESULTS).exists() else []:
        name = path.relative_to(root).as_posix()
        if not path.is_file() or (path.suffix == '.sig' and path.with_suffix('').is_file()):
            continue
        if path.suffix != '.json': raise Invalid(f'{name} is not a result summary or its signature')
        raw, sig = path.read_bytes(), path.with_name(path.name + '.sig')
        if not sig.is_file() or SIGNATURE.fullmatch(sig.read_text().strip()) is None:
            raise Invalid(f'{name} has no signature in hex beside it ({name}.sig)')
        summary = json.loads(raw)
        found = summary_problems(summary, d)
        if found: raise Invalid(f'{name}: {"; ".join(found)}')
        summaries.append({'path': name, 'summary': summary})
        digests += [(name, raw), (f'{name}.sig', sig.read_bytes())]
    for kpi in d['kpis']:
        record = kpi['source'].get('approval')
        if record and record not in records:
            if not record.startswith(f'{APPROVALS}/'): raise Invalid(f'{kpi["id"]}: {record} is not an approval record')
            raw = (root / record).read_bytes()
            records[record] = json.loads(raw)
            digests.append((record, raw))
    approvals = sorted((root / APPROVALS).glob('*.json'))
    decided = [json.loads(path.read_bytes()).get('approved') for path in approvals]
    return {'definitions': d, 'workflow': workflow.decode('utf-8'), 'records': records, 'summaries': summaries,
            'gates': json.loads(gates), 'inputs': [{'path': path, 'sha256': sha256(raw)} for path, raw in digests],
            'approval_records': len(approvals), 'decided': max((day for day in decided if is_date(day)), default=None)}


def gate_items(tracker):
    """The page's gate rows from the tracker: ID, area, status and current blocker."""
    rows = []
    for gate in tracker.get('gates', []):
        if gate.get('status') not in GATE_STATUS: raise Invalid(f'{GATES}: {gate.get("id")} has status {gate.get("status")}')
        blocker = gate.get('current_blocker')
        rows.append({'id': gate['id'], 'name': gate['category'], 'status': GATE_STATUS[gate['status']],
                     'next': blocker if isinstance(blocker, str) and blocker.strip() else None})
    return rows


def summary_problems(summary, definitions):
    """What keeps a result summary off the page: its shape, a KPI the
    definitions do not read from results or whose unit or target differs, or
    the page's public rules (no addresses, no launch server names)."""
    if not isinstance(summary, dict) or summary.get('schema') != RESULT_SCHEMA:
        return [f'not a result summary ({RESULT_SCHEMA})']
    found = []
    prefixes = {'campaign': ('campaign.',), 'run': ('runs.', 'checkpoints.'), 'staging': ('staging.',)}
    prefix = prefixes.get(summary.get('scope'))
    if prefix is None: found.append('the scope is not campaign, run or staging')
    if not is_date(summary.get('date')): found.append('the date is not a date')
    defined = {k['id']: k for k in definitions['kpis']}
    kpis = summary.get('kpis')
    if not isinstance(kpis, list) or not kpis:
        found.append('the summary reports no KPI')
        kpis = []
    for kpi in kpis:
        kid = kpi.get('id') if isinstance(kpi, dict) else None
        spec = defined.get(kid)
        if spec is None or spec['source'] != {'results': True}:
            found.append(f'{kid} is not a KPI the page reads from results')
            continue
        if prefix and not kid.startswith(prefix): found.append(f'{kid} is not a {summary["scope"]} KPI')
        if kpi.get('unit') != spec['unit'] or kpi.get('target') != spec.get('target'):
            found.append(f'{kid}: its unit or target differs from the page\'s')
        if kpi.get('status') not in STATUSES: found.append(f'{kid}: the status is not one of the page\'s')
        value = kpi.get('value')
        if value is not None and not (number(value) and math.isfinite(value)):
            found.append(f'{kid}: the value is not a number')
    for value in strings(summary):
        if contains_address(value): found.append(f'contains an address: {value[:60]}')
        named = [name for name in HOST_NAMES if name in value.lower()]
        if named: found.append(f'names a launch server ({", ".join(named)})')
    return found


def derive(kpi, d, records, proven, summaries=()):
    """A KPI's status, value, date and evidence link (None to keep the
    definitions'), from the source the definitions name."""
    source = kpi['source']
    if set(source) == {'approval'}:
        approved = records[source['approval']].get('approved')
        if not is_date(approved): raise Invalid(f'{kpi["id"]}: {source["approval"]} has no approval date')
        return 'pass', f'Approved {long_date(approved)}', approved, None
    if set(source) == {'tool', 'built', 'remaining'}:
        tool, built, remaining = source['tool'], source['built'], source['remaining']
        if tool != kpi['group']: raise Invalid(f'{kpi["id"]}: its tool is {tool}, not its group {kpi["group"]}')
        if (built is not None) != (tool in proven):
            claim = (f'marked built, but no CI step "({tool})" runs in' if built
                     else 'not marked built, but its CI step runs in')
            raise Invalid(f'{kpi["id"]}: {tool} is {claim} {WORKFLOW}')
        if built is None: return 'planned', None, d['planned_on'], None
        if not is_date(built): raise Invalid(f'{kpi["id"]}: built must be a date')
        value = 'Built and proven in the CI staging network' + (f'; still to come: {remaining}' if remaining else '')
        return ('in_progress' if remaining else 'pass'), value, built, None
    if set(source) == {'results'} and isinstance(source['results'], bool):
        # The newest summary that reports it; summaries are cumulative.
        reports = [(s['summary']['date'], s['path'], k) for s in summaries for k in s['summary']['kpis']
                   if k['id'] == kpi['id']]
        if not reports: return ('planned' if source['results'] else 'not_applicable'), None, d['planned_on'], None
        date, path, result = max(reports, key=lambda r: (r[0], r[1]))
        return result['status'], result['value'], date, f'{REPO}/blob/main/{path}'
    raise Invalid(f'{kpi["id"]}: unknown source {source}')


def build(found, commits):
    """The dashboard, from gather()'s inputs and the commit count."""
    d = found['definitions']
    proven = proven_tools(found['workflow'])
    kpis = []
    for k in d['kpis']:
        status, value, updated, evidence = derive(k, d, found['records'], proven, found['summaries'])
        kpi = {field: k[field] for field in ('id', 'phase', 'group', 'name', 'measures', 'why', 'threshold', 'unit')}
        if 'target' in k: kpi['target'] = k['target']
        kpi.update(status=status, value=value, evidence_url=evidence or k['evidence_url'], updated=updated)
        kpis.append(kpi)
    phases = [dict(p, status=roll_up([k['status'] for k in kpis if k['phase'] == p['id']])) for p in d['phases']]
    as_of = max([k['updated'] for k in kpis] + ([found['decided']] if found.get('decided') else []))
    return {'schema': SCHEMA, 'as_of': as_of,
            'source': {'repo': REPO, 'ref': 'main', 'inputs': found['inputs']},
            'schema_rules': d['schema_rules'],
            'project': {'since': d['project_since'], 'commits_at_as_of': commits,
                        'approval_records': found['approval_records']},
            'phases': phases, 'groups': d['groups'], 'kpis': kpis,
            'gates': {'evidence_url': f'{REPO}/blob/main/{GATES_PAGE}', 'items': gate_items(found['gates'])}}


def render(data): return json.dumps(data, indent=2, ensure_ascii=False) + '\n'


def strings(value):
    if isinstance(value, str): yield value
    elif isinstance(value, dict):
        for item in value.values(): yield from strings(item)
    elif isinstance(value, list):
        for item in value: yield from strings(item)


def problems(data, root=ROOT):
    """The page's checks, and this repository's: no launch server names, evidence
    that exists, and a date no older than the site's own copy."""
    found = [f'page check: {error}' for error in page_problems(data)]
    for value in strings(data):
        named = [name for name in HOST_NAMES if name in value.lower()]
        if named: found.append(f'names a launch server ({", ".join(named)}): {value[:80]}')
    if is_date(data.get('as_of')) and data['as_of'] < SITE_COPY_AS_OF:
        found.append(f'as_of {data["as_of"]} is older than the site\'s own copy ({SITE_COPY_AS_OF})')
    for item in (*data.get('phases', []), *data.get('kpis', []),
                 {'id': 'gates', 'evidence_url': (data.get('gates') or {}).get('evidence_url')}):
        url = item.get('evidence_url') or ''
        if url.startswith(f'{REPO}/blob/main/'):
            path = url[len(f'{REPO}/blob/main/'):].split('#')[0]
            if not (Path(root) / path).exists():
                found.append(f'{item["id"]}: evidence {path} is not in this repository')
    return found


def commit_count(as_of, root=ROOT):
    """The commits on main up to the end of as_of (UTC); the full history is needed."""
    git = ['git', '-C', str(root)]
    shallow = subprocess.run([*git, 'rev-parse', '--is-shallow-repository'], capture_output=True, text=True, check=True)
    if shallow.stdout.strip() != 'false':
        raise Invalid('a shallow clone cannot count the commits; fetch the full history')
    out = subprocess.run([*git, 'rev-list', '--count', f'--before={as_of}T23:59:59Z', 'HEAD'],
                         capture_output=True, text=True, check=True)
    return int(out.stdout)


def stale(root=ROOT):
    """Why the committed file is not what the inputs build; empty when it is current."""
    path = Path(root) / DASHBOARD
    if not path.is_file(): return [f'{DASHBOARD} is missing']
    committed = path.read_text(encoding='utf-8')
    try:
        commits = json.loads(committed)['project']['commits_at_as_of']
    except (ValueError, KeyError, TypeError):
        return [f'{DASHBOARD} is not a dashboard']
    data = build(gather(root), commits)
    found = problems(data, root)
    if render(data) != committed: found.append(f'{DASHBOARD} is stale')
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--write', action='store_true', help='regenerate the file from the inputs')
    args = parser.parse_args()
    try:
        if args.write:
            found = gather()
            data = build(found, 0)
            data = build(found, commit_count(data['as_of']))
            (ROOT / DASHBOARD).write_text(render(data), encoding='utf-8')
        found = stale()
        data = json.loads((ROOT / DASHBOARD).read_text(encoding='utf-8')) if (ROOT / DASHBOARD).is_file() else {}
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as exc:
        print(f'INVALID: {exc}', file=sys.stderr)
        return 2
    for line in found: print(line, file=sys.stderr)
    if found: print('run: python3 node/tools/mainnet-preparation/launch_dashboard.py --write', file=sys.stderr)
    print(json.dumps({'status': 'FAIL' if found else 'PASS', 'as_of': data.get('as_of'),
                      'statuses': {p['id']: p['status'] for p in data.get('phases', [])}, 'problems': len(found)}))
    return 1 if found else 0


if __name__ == '__main__':
    raise SystemExit(main())
