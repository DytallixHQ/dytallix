#!/usr/bin/env python3
"""Resolve E05 values and records into genesis build inputs (E05-d). Never builds or approves genesis.

Each value the builder needs comes from exactly one source: an APPROVED entry
in E05_VALUES.json, or a labeled entry in the proposals file for a value that
is still open. A proposal for an approved value, a missing value, or an
unknown name is refused. The resolution report lists every value's source, so
the build is production-eligible only when every value is approved and every
record accepted; this version builds rehearsals only.
"""
import argparse
import json
from pathlib import Path
import sys

INPUTS_SCHEMA = 'dytallix.genesis-build-inputs.v1'
PROPOSALS_SCHEMA = 'dytallix.genesis-proposals.v1'
RECORDS_SCHEMA = 'dytallix.genesis-records.v1'
RESOLUTION_SCHEMA = 'dytallix.genesis-resolution.v1'
PROPOSAL_STATUSES = {'PROPOSED', 'MEASURE_PLACEHOLDER'}


def integer(value):
    if type(value) is int: return value
    if isinstance(value, str) and value.lstrip('-').isdigit(): return int(value)
    raise ValueError(f'not an integer: {value!r}')


def amount(value):
    number = integer(value)
    if number < 0: raise ValueError(f'negative amount: {value!r}')
    return str(number)


def integers(length):
    def convert(value):
        if not isinstance(value, list) or len(value) != length: raise ValueError(f'expected {length} integers')
        return [integer(x) for x in value]
    return convert


def costs(value):
    if not isinstance(value, dict) or set(value) != {'mldsa65'}: raise ValueError('expected {"mldsa65": cost}')
    return {'mldsa65': integer(value['mldsa65'])}


def bounds(convert):
    def inner(value):
        if not isinstance(value, dict) or set(value) != {'min', 'max'}: raise ValueError('expected {min, max}')
        return {'min': convert(value['min']), 'max': convert(value['max'])}
    return inner


def algorithms(value):
    if not isinstance(value, list) or not value or not all(isinstance(x, str) for x in value): raise ValueError('expected algorithm names')
    return sorted(set(value))


def key_sizes(value):
    if not isinstance(value, dict): raise ValueError('expected {algorithm: key bytes}')
    return {k: integer(v) for k, v in sorted(value.items())}


# E05 value name -> (location in the inputs file, conversion). Derived values
# are absent: the builder computes them from approved rules.
VALUES = {
    'app_max_tx_bytes': (('application', 'max_tx_bytes'), integer),
    'app_max_block_bytes': (('application', 'max_block_bytes'), integer),
    'app_max_txs': (('application', 'max_txs'), integer),
    'min_self_bond': (('lifecycle', 'min_self_bond'), amount),
    'max_active': (('lifecycle', 'max_active'), integer),
    'evidence_max_age_blocks': (('lifecycle', 'evidence_max_age_blocks'), integer),
    'evidence_max_age_seconds': (('lifecycle', 'evidence_max_age_seconds'), integer),
    'processing_margin_blocks': (('lifecycle', 'processing_margin_blocks'), integer),
    'processing_margin_seconds': (('lifecycle', 'processing_margin_seconds'), integer),
    'penalty_numerator': (('penalty', 'numerator'), integer),
    'penalty_denominator': (('penalty', 'denominator'), integer),
    'recovery_gas_price': (('recovery_profile', 'gas_price'), integer),
    'recovery_minimum_gas': (('recovery_profile', 'minimum_gas'), integer),
    'recovery_max_transaction_gas': (('recovery_profile', 'max_transaction_gas'), integer),
    'recovery_max_block_gas': (('recovery_profile', 'max_block_gas'), integer),
    'recovery_max_block_recovery_bytes': (('recovery_profile', 'max_block_recovery_bytes'), integer),
    'recovery_max_block_recovery_signatures': (('recovery_profile', 'max_block_recovery_signatures'), integer),
    'recovery_max_pending_accounts': (('recovery_profile', 'max_pending_accounts'), integer),
    'recovery_max_due_expiry_events_per_height': (('recovery_profile', 'max_due_expiry_events_per_height'), integer),
    'recovery_mandatory_expiry_gas_budget': (('recovery_profile', 'mandatory_expiry_gas_budget'), integer),
    'recovery_expiry_event_gas_cost': (('recovery_profile', 'expiry_event_gas_cost'), integer),
    'recovery_action_costs': (('recovery_profile', 'action_costs'), integers(9)),
    'recovery_wire_byte_cost': (('recovery_profile', 'wire_byte_cost'), integer),
    'recovery_read_byte_cost': (('recovery_profile', 'read_byte_cost'), integer),
    'recovery_write_byte_cost': (('recovery_profile', 'write_byte_cost'), integer),
    'recovery_signature_costs': (('recovery_profile', 'signature_costs'), costs),
    'ordinary_gas_price': (('ordinary', 'gas_price'), integer),
    'ordinary_minimum_gas': (('ordinary', 'minimum_gas'), integer),
    'ordinary_max_transaction_gas': (('ordinary', 'max_transaction_gas'), integer),
    'ordinary_max_block_transaction_gas': (('ordinary', 'max_block_transaction_gas'), integer),
    'ordinary_max_block_transaction_bytes': (('ordinary', 'max_block_transaction_bytes'), integer),
    'ordinary_max_block_signature_checks': (('ordinary', 'max_block_signature_checks'), integer),
    'ordinary_max_wire_bytes': (('ordinary', 'limits', 'max_wire_bytes'), integer),
    'ordinary_max_actions': (('ordinary', 'limits', 'max_actions'), integer),
    'ordinary_max_identifier_bytes': (('ordinary', 'limits', 'max_identifier_bytes'), integer),
    'ordinary_max_data_bytes': (('ordinary', 'limits', 'max_data_bytes'), integer),
    'ordinary_max_memo_bytes': (('ordinary', 'limits', 'max_memo_bytes'), integer),
    'ordinary_max_consensus_key_bytes': (('ordinary', 'limits', 'max_consensus_key_bytes'), integer),
    'ordinary_max_proof_bytes': (('ordinary', 'limits', 'max_proof_bytes'), integer),
    'ordinary_max_expiry_lifetime': (('ordinary', 'limits', 'max_expiry_lifetime'), integer),
    'ordinary_allowed_algorithms': (('ordinary', 'limits', 'allowed_algorithms'), algorithms),
    'ordinary_transaction_overhead': (('ordinary', 'transaction_overhead'), integer),
    'ordinary_receipt_metadata_cost': (('ordinary', 'receipt_metadata_cost'), integer),
    'ordinary_wire_byte_cost': (('ordinary', 'wire_byte_cost'), integer),
    'ordinary_read_byte_cost': (('ordinary', 'read_byte_cost'), integer),
    'ordinary_write_byte_cost': (('ordinary', 'write_byte_cost'), integer),
    'ordinary_action_costs': (('ordinary', 'action_costs'), integers(12)),
    'ordinary_signature_costs': (('ordinary', 'signature_costs'), costs),
    'ordinary_validator_proof_costs': (('ordinary', 'validator_proof_costs'), costs),
    'account_creation_fee_udrt': (('ordinary', 'account_creation_fee_udrt'), amount),
    'ordinary_max_state_bytes': (('ordinary', 'max_state_bytes'), integer),
    'ordinary_max_grants': (('ordinary', 'max_grants'), integer),
    'ordinary_max_receipts': (('ordinary', 'max_receipts'), integer),
    'ordinary_max_retained_profiles': (('ordinary', 'max_retained_profiles'), integer),
    'queue_max_entries': (('ordinary', 'queue_max_entries'), integer),
    'queue_max_wire_bytes': (('ordinary', 'queue_max_wire_bytes'), integer),
    'queue_max_signature_work': (('ordinary', 'queue_max_signature_work'), integer),
    'template_timing_version': (('account_template', 'timing_version'), integer),
    'template_recovery_delay': (('account_template', 'recovery_delay'), integer),
    'template_finalization_window': (('account_template', 'finalization_window'), integer),
    'template_policy_delay': (('account_template', 'policy_delay'), integer),
    'template_policy_window': (('account_template', 'policy_window'), integer),
    'template_submission_lifetime': (('account_template', 'submission_lifetime'), integer),
    'template_algorithms': (('account_template', 'algorithms'), key_sizes),
    'governance_max_governance_action_bytes': (('governance', 'max_governance_action_bytes'), integer),
    'governance_action_costs': (('governance', 'governance_action_costs'), integers(3)),
    'quorum_bps': (('governance', 'quorum_bps'), integer),
    'approval_bps': (('governance', 'approval_bps'), integer),
    'veto_bps': (('governance', 'veto_bps'), integer),
    'voting_period_blocks': (('governance', 'voting_period_blocks'), integer),
    'timelock_blocks': (('governance', 'timelock_blocks'), integer),
    'max_voters': (('governance', 'max_voters'), integer),
    'deposit_period_blocks': (('governance', 'deposit_period_blocks'), integer),
    'minimum_deposit_udgt': (('governance', 'minimum_deposit_udgt'), amount),
    'max_action_bytes': (('governance', 'max_action_bytes'), integer),
    'max_depositors': (('governance', 'max_depositors'), integer),
    'class_parameter_change_max_data_bytes': (('governance', '@class1'), integer),
    'class_validator_registry_max_data_bytes': (('governance', '@class2'), integer),
    'bounds_gas_price': (('governance', 'bounds', 'gas_price'), bounds(integer)),
    'bounds_resource_cost': (('governance', 'bounds', 'resource_cost'), bounds(integer)),
    'bounds_account_creation_fee_udrt': (('governance', 'bounds', 'account_creation_fee_udrt'), bounds(amount)),
    'bounds_reference_send_fee_udrt': (('governance', 'bounds', 'reference_send_fee_udrt'), bounds(amount)),
    'bounds_min_self_bond': (('governance', 'bounds', 'min_self_bond'), bounds(amount)),
    'bounds_max_active': (('governance', 'bounds', 'max_active'), bounds(integer)),
    'reward_max_positions': (('reward', 'max_positions'), integer),
    'epoch_blocks': (('issuance', 'epoch_blocks'), integer),
    'max_recorded_epochs': (('issuance', 'max_recorded_epochs'), integer),
    'initial_epoch_budget_udrt': (('issuance', 'initial_epoch_budget_udrt'), amount),
    'controller_target_ppm': (('issuance', 'controller', 'target_ppm'), integer),
    'controller_shock_threshold_ppm': (('issuance', 'controller', 'shock_threshold_ppm'), integer),
    'controller_volatility_threshold_ppm': (('issuance', 'controller', 'volatility_threshold_ppm'), integer),
    'controller_window_samples': (('issuance', 'controller', 'window_samples'), integer),
    'controller_integral_min': (('issuance', 'controller', 'integral_min'), integer),
    'controller_integral_max': (('issuance', 'controller', 'integral_max'), integer),
    'controller_soft_proportional': (('issuance', 'controller', 'soft', 'proportional'), integer),
    'controller_soft_integral': (('issuance', 'controller', 'soft', 'integral'), integer),
    'controller_soft_derivative': (('issuance', 'controller', 'soft', 'derivative'), integer),
    'controller_hard_proportional': (('issuance', 'controller', 'hard', 'proportional'), integer),
    'controller_hard_integral': (('issuance', 'controller', 'hard', 'integral'), integer),
    'controller_hard_derivative': (('issuance', 'controller', 'hard', 'derivative'), integer),
    'controller_base_udrt': (('issuance', 'controller', 'base_udrt'), integer),
    'controller_min_udrt': (('issuance', 'controller', 'min_udrt'), integer),
    'controller_max_udrt': (('issuance', 'controller', 'max_udrt'), integer),
    'engine_block_max_bytes': (('engine', 'block_max_bytes'), integer),
    'engine_block_max_gas': (('engine', 'block_max_gas'), integer),
    'engine_evidence_max_bytes': (('engine', 'evidence_max_bytes'), integer),
    'drt_bootstrap_total_udrt': (('drt_bootstrap_total_udrt',), amount),
    'emergency_max_validity_blocks': (('root', 'emergency', 'max_validity_blocks'), integer),
    'emergency_max_anchor_age_blocks': (('root', 'emergency', 'max_anchor_age_blocks'), integer),
    'migration_max_receipts': (('root', 'upgrade', 'migration_bounds', 'max_receipts'), integer),
    'migration_max_receipt_bytes': (('root', 'upgrade', 'migration_bounds', 'max_receipt_bytes'), integer),
    'migration_max_write_bytes': (('root', 'upgrade', 'migration_bounds', 'max_write_bytes'), integer),
    'handover_authority_threshold': (('root', 'handover', 'authority', 'threshold'), integer),
    'handover_max_signatures': (('root', 'handover', 'max_signatures'), integer),
}


def place(tree, path, value):
    for key in path[:-1]: tree = tree.setdefault(key, {})
    tree[path[-1]] = value


def exact(obj, keys, where):
    if not isinstance(obj, dict) or set(obj) != set(keys): raise ValueError(f'{where}: exact fields {sorted(keys)} required')


def resolve(values, proposals, records):
    """Return (inputs, resolution). Raises ValueError on any unresolved or conflicting value."""
    if proposals.get('schema') != PROPOSALS_SCHEMA: raise ValueError('proposals schema mismatch')
    if records.get('schema') != RECORDS_SCHEMA: raise ValueError('records schema mismatch')
    entries = {v['name']: v for v in values['values']}
    offered = proposals['values']
    unknown = sorted(set(offered) - set(entries))
    if unknown: raise ValueError(f'proposals name unknown values: {unknown}')
    stale = sorted(n for n in offered if entries[n]['status'] == 'APPROVED')
    if stale: raise ValueError(f'proposals for approved values: {stale}')
    unused = sorted(set(offered) - set(VALUES))
    if unused: raise ValueError(f'proposals the builder does not use: {unused}')
    inputs, resolution = {}, []
    for name, (path, convert) in VALUES.items():
        entry = entries.get(name)
        if entry is None: raise ValueError(f'{name}: not in E05_VALUES.json')
        if entry['status'] == 'APPROVED':
            raw, source, reference = entry['approved'], 'APPROVED', entry['approval_record']
        elif name in offered:
            proposal = offered[name]
            exact(proposal, {'value', 'status', 'basis'}, f'proposal {name}')
            if proposal['status'] not in PROPOSAL_STATUSES: raise ValueError(f'{name}: proposal status must be one of {sorted(PROPOSAL_STATUSES)}')
            raw, source, reference = proposal['value'], proposal['status'], 'genesis/PROPOSALS.json'
        else:
            raise ValueError(f'{name}: open with no proposal')
        try: value = convert(raw)
        except (ValueError, TypeError) as exc: raise ValueError(f'{name}: {exc}') from None
        place(inputs, path, value)
        resolution.append({'name': name, 'source': source, 'reference': reference, 'value': value})

    exact(records, {'schema', 'status', 'boundary', 'chain_id', 'genesis_time', 'network', 'accounts', 'validators', 'delegations', 'governance_action_classes', 'root'}, 'records')
    if records['status'] not in {'SYNTHETIC', 'ACCEPTED'}: raise ValueError('records status must be SYNTHETIC or ACCEPTED')
    governance = inputs['governance']
    classes = {c['class']: c['approval_digest'] for c in records['governance_action_classes']}
    if set(classes) != {1, 2}: raise ValueError('records need approval digests for action classes 1 and 2')
    governance['action_classes'] = [
        {'class': 1, 'max_data_bytes': governance.pop('@class1'), 'approval_digest': classes[1]},
        {'class': 2, 'max_data_bytes': governance.pop('@class2'), 'approval_digest': classes[2]},
    ]
    root = records['root']
    exact(root, {'release_sha512', 'emergency', 'upgrade', 'handover'}, 'records.root')
    built_root = inputs['root']
    built_root['release_sha512'] = root['release_sha512']
    exact(root['emergency'], {'authority_epoch', 'freeze', 'resume'}, 'records.root.emergency')
    built_root['emergency'].update(root['emergency'])
    exact(root['upgrade'], {'parameter_set', 'authority_epoch', 'authority'}, 'records.root.upgrade')
    if root['upgrade']['parameter_set'] != 'SLH-DSA-SHAKE-256s': raise ValueError('upgrade authority must use SLH-DSA-SHAKE-256s')
    built_root['upgrade']['authority_epoch'] = root['upgrade']['authority_epoch']
    built_root['upgrade']['authority'] = root['upgrade']['authority']
    exact(root['handover'], {'authority_epoch', 'keys'}, 'records.root.handover')
    built_root['handover']['authority_epoch'] = root['handover']['authority_epoch']
    built_root['handover']['authority']['keys'] = root['handover']['keys']
    for field in ('chain_id', 'genesis_time', 'network', 'accounts', 'validators', 'delegations'):
        inputs[field] = records[field]
    inputs.update({'schema': INPUTS_SCHEMA, 'mode': 'rehearsal'})
    for field in ('chain_id', 'genesis_time', 'network', 'accounts', 'validators', 'delegations', 'governance_action_classes', 'root'):
        resolution.append({'name': 'record:' + field, 'source': 'RECORD_' + records['status'], 'reference': 'records', 'value': None})
    counts = {}
    for item in resolution: counts[item['source']] = counts.get(item['source'], 0) + 1
    report = {
        'schema': RESOLUTION_SCHEMA,
        'production_eligible': False,
        'boundary': 'Rehearsal only: the node accepts only local-qualification and development profiles until production activation. Production also needs every value APPROVED and every record ACCEPTED.',
        'counts': dict(sorted(counts.items())),
        'values': resolution,
    }
    return inputs, report


def render(value): return json.dumps(value, indent=2, sort_keys=True) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--values', type=Path, required=True)
    parser.add_argument('--proposals', type=Path, required=True)
    parser.add_argument('--records', type=Path, required=True)
    parser.add_argument('--inputs', type=Path, required=True, help='the genesis build inputs to write or check')
    parser.add_argument('--resolution', type=Path, required=True, help='the resolution report to write or check')
    parser.add_argument('--check', action='store_true', help='compare with the existing files instead of writing')
    args = parser.parse_args()
    try:
        inputs, report = resolve(*(json.loads(p.read_text()) for p in (args.values, args.proposals, args.records)))
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f'UNRESOLVED: {exc}', file=sys.stderr); return 2
    outputs = [(args.inputs, render(inputs)), (args.resolution, render(report))]
    if args.check:
        stale = [str(p) for p, text in outputs if not p.is_file() or p.read_text() != text]
        if stale: print('STALE: ' + ', '.join(stale), file=sys.stderr); return 1
    else:
        for path, text in outputs: path.write_text(text)
    print(json.dumps({'status': 'RESOLVED', 'production_eligible': False, 'counts': report['counts']}))
    return 0


if __name__ == '__main__': raise SystemExit(main())
