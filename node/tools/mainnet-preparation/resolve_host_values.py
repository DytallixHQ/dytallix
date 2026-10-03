#!/usr/bin/env python3
"""Resolve E05 values into the host configuration generator's values (E05). Never approves a value.

Each per-host engine setting comes from exactly one source: an APPROVED entry
in E05_VALUES.json, or a labeled entry in launch/hosts/PROPOSALS.json for a
value that is still open. A proposal for an approved value, a missing value or
an unknown name is refused. The resolution report lists every value's source,
so the host values are production-eligible only when every value is approved.
`dytallix-host-config` reads the values file with the pin plan and the engine
genesis.
"""
import argparse
import json
from pathlib import Path
import re
import sys

VALUES_SCHEMA = 'dytallix.host-values.v1'
PROPOSALS_SCHEMA = 'dytallix.host-proposals.v1'
RESOLUTION_SCHEMA = 'dytallix.host-values-resolution.v1'
PROPOSAL_STATUSES = {'PROPOSED', 'MEASURE_PLACEHOLDER'}
# A Go duration with one unit, as the approved values write them.
DURATION = re.compile(r'^(0|[1-9][0-9]*)(ns|us|ms|s|m|h)$')
ROLES = ('validator', 'sentry', 'endpoint')


def integer(value):
    if type(value) is not int or value < 0: raise ValueError(f'not a non-negative integer: {value!r}')
    return value


def boolean(value):
    if type(value) is not bool: raise ValueError(f'not a boolean: {value!r}')
    return value


def duration(value):
    if not isinstance(value, str) or DURATION.fullmatch(value) is None: raise ValueError(f'not a duration: {value!r}')
    return value


def per_role(value):
    if not isinstance(value, dict) or set(value) != set(ROLES): raise ValueError(f'expected {{{", ".join(ROLES)}}}')
    return {role: integer(value[role]) for role in ROLES}


# E05 value name -> (location in the host values, conversion).
VALUES = {
    'timeout_propose': (('consensus', 'timeout_propose'), duration),
    'timeout_propose_delta': (('consensus', 'timeout_propose_delta'), duration),
    'timeout_prevote': (('consensus', 'timeout_prevote'), duration),
    'timeout_prevote_delta': (('consensus', 'timeout_prevote_delta'), duration),
    'timeout_precommit': (('consensus', 'timeout_precommit'), duration),
    'timeout_precommit_delta': (('consensus', 'timeout_precommit_delta'), duration),
    'timeout_commit': (('consensus', 'timeout_commit'), duration),
    'skip_timeout_commit': (('consensus', 'skip_timeout_commit'), boolean),
    'create_empty_blocks': (('consensus', 'create_empty_blocks'), boolean),
    'create_empty_blocks_interval': (('consensus', 'create_empty_blocks_interval'), duration),
    'peer_gossip_sleep_duration': (('consensus', 'peer_gossip_sleep_duration'), duration),
    'peer_query_maj23_sleep_duration': (('consensus', 'peer_query_maj23_sleep_duration'), duration),
    'block_time_tolerance': (('consensus', 'block_time_tolerance'), duration),
    'double_sign_check_height': (('consensus', 'double_sign_check_height'), per_role),
    'mempool_size': (('mempool', 'size'), integer),
    'mempool_max_txs_bytes': (('mempool', 'max_txs_bytes'), integer),
    'mempool_cache_size': (('mempool', 'cache_size'), integer),
    'mempool_recheck_timeout': (('mempool', 'recheck_timeout'), duration),
    'mempool_max_tx_bytes': (('mempool', 'max_tx_bytes'), integer),
    'p2p_max_num_inbound_peers': (('p2p', 'max_num_inbound_peers'), integer),
    'p2p_max_num_outbound_peers': (('p2p', 'max_num_outbound_peers'), integer),
    'p2p_max_packet_msg_payload_size': (('p2p', 'max_packet_msg_payload_size'), integer),
    'p2p_send_rate': (('p2p', 'send_rate'), integer),
    'p2p_recv_rate': (('p2p', 'recv_rate'), integer),
    'p2p_flush_throttle_timeout': (('p2p', 'flush_throttle_timeout'), duration),
    'p2p_handshake_timeout': (('p2p', 'handshake_timeout'), duration),
    'p2p_dial_timeout': (('p2p', 'dial_timeout'), duration),
    'p2p_persistent_peers_max_dial_period': (('p2p', 'persistent_peers_max_dial_period'), duration),
    'pqc_handshake_timeout_ms': (('transport', 'handshake_timeout_ms'), integer),
    'statesync_discovery_time': (('statesync', 'discovery_time'), duration),
    'statesync_chunk_request_timeout': (('statesync', 'chunk_request_timeout'), duration),
    'statesync_chunk_fetchers': (('statesync', 'chunk_fetchers'), integer),
    'statesync_max_snapshot_chunks': (('statesync', 'max_snapshot_chunks'), integer),
    'statesync_trust_period': (('statesync', 'trust_period'), duration),
}


def exact(obj, keys, where):
    if not isinstance(obj, dict) or set(obj) != set(keys): raise ValueError(f'{where}: exact fields {sorted(keys)} required')


def resolve(values, proposals):
    """Return (host values, resolution). Raises ValueError on any unresolved or conflicting value."""
    exact(proposals, {'schema', 'rule', 'as_of', 'values'}, 'proposals')
    if proposals['schema'] != PROPOSALS_SCHEMA: raise ValueError('proposals schema mismatch')
    entries = {v['name']: v for v in values['values']}
    offered = proposals['values']
    unknown = sorted(set(offered) - set(entries))
    if unknown: raise ValueError(f'proposals name unknown values: {unknown}')
    stale = sorted(n for n in offered if entries[n]['status'] == 'APPROVED')
    if stale: raise ValueError(f'proposals for approved values: {stale}')
    unused = sorted(set(offered) - set(VALUES))
    if unused: raise ValueError(f'proposals the host configuration does not use: {unused}')
    host, resolution = {'schema': VALUES_SCHEMA}, []
    for name, ((section, field), convert) in VALUES.items():
        entry = entries.get(name)
        if entry is None: raise ValueError(f'{name}: not in E05_VALUES.json')
        if entry['status'] == 'APPROVED':
            raw, source, reference = entry['approved'], 'APPROVED', entry['approval_record']
        elif name in offered:
            proposal = offered[name]
            exact(proposal, {'value', 'status', 'basis'}, f'proposal {name}')
            if proposal['status'] not in PROPOSAL_STATUSES: raise ValueError(f'{name}: proposal status must be one of {sorted(PROPOSAL_STATUSES)}')
            raw, source, reference = proposal['value'], proposal['status'], 'hosts/PROPOSALS.json'
        else:
            raise ValueError(f'{name}: open with no proposal')
        try: value = convert(raw)
        except (ValueError, TypeError) as exc: raise ValueError(f'{name}: {exc}') from None
        host.setdefault(section, {})[field] = value
        resolution.append({'name': name, 'source': source, 'reference': reference, 'value': value})
    counts = {}
    for item in resolution: counts[item['source']] = counts.get(item['source'], 0) + 1
    report = {
        'schema': RESOLUTION_SCHEMA,
        'production_eligible': set(counts) == {'APPROVED'},
        'boundary': 'Production eligibility needs every value APPROVED. Eligibility accepts nothing: the pin plan records, the engine genesis, the reviewed host files and gate acceptance remain required.',
        'counts': dict(sorted(counts.items())),
        'values': resolution,
    }
    return host, report


def render(value): return json.dumps(value, indent=2, sort_keys=True) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--values', type=Path, required=True)
    parser.add_argument('--proposals', type=Path, required=True)
    parser.add_argument('--host-values', type=Path, required=True, help='the host values to write or check')
    parser.add_argument('--resolution', type=Path, required=True, help='the resolution report to write or check')
    parser.add_argument('--check', action='store_true', help='compare with the existing files instead of writing')
    args = parser.parse_args()
    try:
        host, report = resolve(*(json.loads(p.read_text()) for p in (args.values, args.proposals)))
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f'UNRESOLVED: {exc}', file=sys.stderr); return 2
    outputs = [(args.host_values, render(host)), (args.resolution, render(report))]
    if args.check:
        stale = [str(p) for p, text in outputs if not p.is_file() or p.read_text() != text]
        if stale: print('STALE: ' + ', '.join(stale), file=sys.stderr); return 1
    else:
        for path, text in outputs: path.write_text(text)
    print(json.dumps({'status': 'RESOLVED', 'production_eligible': report['production_eligible'], 'counts': report['counts']}))
    return 0


if __name__ == '__main__': raise SystemExit(main())
