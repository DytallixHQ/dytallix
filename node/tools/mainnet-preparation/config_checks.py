"""Independent review of the full application configuration and engine genesis (E05-d2).

The node checks these files with its own code; this module re-derives the
same rules from the published formats in Python, so a builder error and a
node error would have to agree to pass both. It reviews structure and
cross-file bindings only: no signature, key possession or approval.
"""
import base64
import hashlib
import json
import re
import native_checks as n

I64_MAX = 2**63 - 1
LIFECYCLE_PROFILE = 'cometbft-lifecycle-local-qualification'
PENALTY_PROFILE = 'cometbft-penalty-local-qualification'
SECTIONS = ('lifecycle', 'penalty', 'recovery', 'ordinary', 'governance', 'emergency', 'upgrade', 'release_handover')
BASE_FIELDS = 'profile engine chain_id app_state_sha256 gas_price max_tx_bytes max_block_bytes max_txs validators'.split()
CONTROL_SIGNATURE_HEX = 2 * 29_792  # one hex SLH-DSA-SHAKE-256s signature
TRANSPORT_OVERHEAD = 43  # {"type":"ordinary_v2","envelope_base64":""}
MLDSA65 = {'mldsa65': 1952}
ORIGIN_DOMAIN = b'DYTALLIX/ACCOUNT-ORIGIN/V1\0'
HRP = {1: 'dytallix', 2: 'tdytallix', 3: 'ddytallix'}
ENTRY_POLICY = {'proposer_eligibility': 'registered_owner_with_effective_bond_at_finalized_parent',
                'validator_voting': 'own_effective_bond_only', 'vote_delegation': 'disabled', 'cancellation': 'disabled'}
HEX64 = re.compile(r'[0-9a-f]{64}')
HEX128 = re.compile(r'[0-9a-f]{128}')
GENESIS_TIME = re.compile(r'\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])T([01]\d|2[0-3]):[0-5]\d:[0-5]\dZ')


def dec(value, maximum=n.U64_MAX):
    """A u64 serialized as a canonical decimal string."""
    number = n.amount(value)
    n.require(number <= maximum, 'Decimal value exceeds its bound')
    return number


def positive(value, maximum=n.U64_MAX): return n.require(n.uint(value, maximum) > 0, 'Positive value required') or value


def byte_array(value, size):
    n.require(type(value) is list and len(value) == size and all(type(b) is int and 0 <= b <= 255 for b in value), 'Byte array expected')
    return bytes(value)


# Bech32m (BIP 350), for account addresses.
CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l'


def _polymod(values):
    generator = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]
    check = 1
    for value in values:
        top = check >> 25
        check = (check & 0x1ffffff) << 5 ^ value
        for i in range(5):
            check ^= generator[i] if (top >> i) & 1 else 0
    return check


def bech32m(hrp, payload):
    data, acc, bits = [], 0, 0
    for byte in payload:
        acc = acc << 8 | byte
        bits += 8
        while bits >= 5:
            bits -= 5
            data.append(acc >> bits & 31)
    if bits: data.append(acc << (5 - bits) & 31)
    expanded = [ord(c) >> 5 for c in hrp] + [0] + [ord(c) & 31 for c in hrp]
    mod = _polymod(expanded + data + [0] * 6) ^ 0x2bc830a3
    return hrp + '1' + ''.join(CHARSET[d] for d in data + [mod >> 5 * (5 - i) & 31 for i in range(6)])


def origin_account(network, chain, public_key):
    """The account ID and address an ML-DSA-65 origin key derives (address v1)."""
    origin = ORIGIN_DOMAIN + bytes([network]) + len(chain).to_bytes(2, 'big') + chain.encode() + (1).to_bytes(2, 'big') + len(public_key).to_bytes(4, 'big') + public_key
    account_id = hashlib.sha3_256(origin).digest()
    return account_id, bech32m(HRP[network], bytes([1, 1]) + account_id)


def validator_profile_digest(lifecycle):
    """SHA3-256 of the lifecycle role, as ordinary_state::validator_profile_digest builds it."""
    role = dict(lifecycle)
    role.update({'approved_operators': {}, 'min_self_bond': '0', 'max_active': 0})
    encoded = json.dumps(role, separators=(',', ':'), ensure_ascii=False).encode()
    data = b'DYTALLIX/ORDINARY-VALIDATOR-PROOF-PROFILE\0' + (1).to_bytes(2, 'big') + (1).to_bytes(2, 'big') + (1952).to_bytes(4, 'big') + (3309).to_bytes(4, 'big') + b'PURE-ML-DSA/EMPTY-CONTEXT\0' + len(encoded).to_bytes(4, 'big') + encoded
    return hashlib.sha3_256(data).digest()


def lifecycle(config):
    life = config['lifecycle']
    n.exact(life, 'version profile chain_id approved_operators min_self_bond max_active evidence_max_age_blocks evidence_max_age_seconds processing_margin_blocks processing_margin_seconds')
    n.require(life['version'] == 1 and life['profile'] == LIFECYCLE_PROFILE and life['chain_id'] == config['chain_id'], 'lifecycle_identity_mismatch')
    operators = life['approved_operators']
    n.require(type(operators) is dict and 1 <= len(operators) <= 64, 'lifecycle_operator_bound')
    for validator, owner in operators.items(): n.identity(validator); n.identity(owner)
    n.require(0 < n.amount(life['min_self_bond']) <= 2**60 - 1, 'lifecycle_self_bond_bound')
    n.require(len(config['validators']) <= n.uint(life['max_active']) <= 64, 'lifecycle_active_bound')
    for key in ('evidence_max_age_blocks', 'evidence_max_age_seconds', 'processing_margin_blocks', 'processing_margin_seconds'): positive(life[key])
    # E05-a rule 7: the age plus margin fits a signed nanosecond duration.
    n.require(life['evidence_max_age_seconds'] + life['processing_margin_seconds'] <= I64_MAX // 10**9, 'evidence_seconds_bound')
    n.require(all(v['reward_address'] in operators for v in config['validators']), 'validator_not_an_approved_operator')


def penalty(config):
    if config['profile'] != PENALTY_PROFILE:
        n.require('penalty' not in config, 'penalty_without_penalty_profile'); return
    p = config['penalty']
    n.exact(p, 'version profile chain_id penalty_numerator penalty_denominator production_activation')
    n.require(p['version'] == 1 and p['profile'] == PENALTY_PROFILE and p['chain_id'] == config['chain_id'], 'penalty_identity_mismatch')
    n.require(0 < n.uint(p['penalty_numerator']) <= n.uint(p['penalty_denominator']), 'penalty_rate_bound')
    n.require(p['production_activation'] is False, 'penalty_production_activation')


def recovery(config, app_digest):
    book = config['recovery']
    n.exact(book, 'version last_height profile accounts sponsor_receipts operation_success expiry_index origins')
    n.require(book['version'] == 1 and book['last_height'] == 0 and book['sponsor_receipts'] == {} and book['operation_success'] == {} and book['expiry_index'] == {}, 'recovery_not_fresh_genesis')
    p = book['profile']
    n.exact(p, 'version activation_height denomination gas_price minimum_gas max_transaction_gas max_block_gas max_block_recovery_bytes max_block_recovery_signatures max_fee_cap max_pending_accounts max_due_expiry_events_per_height mandatory_expiry_gas_budget expiry_event_gas_cost action_costs wire_byte_cost read_byte_cost write_byte_cost signature_costs')
    n.require(p['denomination'] == 'udrt' and positive(p['gas_price']) and positive(p['max_transaction_gas'], I64_MAX), 'recovery_profile_invalid')
    n.require(n.uint(p['minimum_gas']) <= p['max_transaction_gas'] <= n.uint(p['max_block_gas'], I64_MAX), 'recovery_gas_order')
    for key in ('max_block_recovery_bytes', 'max_block_recovery_signatures', 'max_pending_accounts', 'max_due_expiry_events_per_height', 'mandatory_expiry_gas_budget'): positive(p[key])
    n.require(type(p['action_costs']) is list and len(p['action_costs']) == 9 and all(type(x) is int and x >= 0 for x in p['action_costs']), 'recovery_action_costs')
    n.require(type(p['signature_costs']) is dict and 1 <= len(p['signature_costs']) <= 2 and set(p['signature_costs']) <= {'mldsa65', 'mldsa87'}, 'recovery_signature_costs')
    # E05-a rule 3: the cap covers the largest fee a sponsor signs.
    n.require(type(p['max_fee_cap']) is int and p['max_fee_cap'] >= p['max_transaction_gas'] * p['gas_price'], 'recovery_fee_cap_below_largest_fee')
    accounts = book['accounts']
    n.require(type(accounts) is dict and accounts, 'recovery_accounts_missing')
    addresses, networks = {}, set()
    for key, account in accounts.items():
        n.exact(account, 'address recovery sponsor_nonce')
        state = account['recovery']
        n.exact(state, 'domain config active_key active_generation spending_nonce policy policy_version recovery_sequence policy_change_sequence status pending_recovery pending_policy last_height')
        domain = state['domain']; n.exact(domain, 'network chain_id genesis_digest account_id')
        n.require(domain['network'] in HRP and domain['chain_id'] == config['chain_id'], 'recovery_domain_mismatch')
        n.require(byte_array(domain['genesis_digest'], 32).hex() == app_digest, 'recovery_genesis_digest_mismatch')
        account_id = byte_array(domain['account_id'], 32)
        n.require(key == account_id.hex(), 'recovery_account_key_mismatch')
        n.exact(state['config'], 'timing_version recovery_delay finalization_window policy_delay policy_window submission_lifetime algorithms')
        n.require(state['config']['algorithms'] == MLDSA65 and all(n.uint(v) > 0 for k, v in state['config'].items() if k != 'algorithms'), 'recovery_config_invalid')
        n.exact(state['active_key'], 'algorithm public_key')
        n.require(state['active_key']['algorithm'] == 'mldsa65', 'recovery_key_algorithm')
        public_key = byte_array(state['active_key']['public_key'], 1952)
        derived_id, derived_address = origin_account(domain['network'], domain['chain_id'], public_key)
        n.require(derived_id == account_id and account['address'] == derived_address, 'account_address_not_derived_from_origin_key')
        n.require(account['sponsor_nonce'] == 0 and state['active_generation'] == 0 and state['spending_nonce'] == 0 and state['last_height'] == 0, 'recovery_counters_not_zero')
        n.require(state['policy'] is None and state['policy_version'] == state['recovery_sequence'] == state['policy_change_sequence'] == 0 and state['status'] == 'Normal' and state['pending_recovery'] is None and state['pending_policy'] is None, 'recovery_state_not_fresh')
        n.require(book['origins'].get(key) == state['active_key'], 'recovery_origin_mismatch')
        n.require(account['address'] not in addresses, 'duplicate_recovery_address')
        addresses[account['address']] = key; networks.add(domain['network'])
    n.require(len(networks) == 1 and set(book['origins']) == set(accounts), 'recovery_origins_or_network_mismatch')
    return set(addresses), networks.pop()


def ordinary(config, network, recovery_addresses):
    o = config['ordinary']
    n.exact(o, 'version fee_profile account_template initial_grants max_state_bytes max_grants max_receipts max_retained_profiles max_transport_bytes queue_max_entries queue_max_wire_bytes queue_max_signature_work')
    n.require(o['version'] == 1, 'ordinary_version')
    f = o['fee_profile']
    n.exact(f, 'ordinary_fee_contract_version version activation_height denomination gas_price minimum_gas max_transaction_gas max_block_transaction_gas max_block_transaction_bytes max_block_signature_checks max_fee_cap limits transaction_overhead receipt_metadata_cost wire_byte_cost read_byte_cost write_byte_cost action_costs signature_costs validator_proof_profile_digest validator_proof_costs account_creation_fee_udrt')
    n.require(f['ordinary_fee_contract_version'] == 1 and f['denomination'] == 'udrt' and dec(f['version']) > 0, 'ordinary_profile_identity')
    gas_price, minimum, max_tx, max_block = (dec(f[k], I64_MAX) for k in ('gas_price', 'minimum_gas', 'max_transaction_gas', 'max_block_transaction_gas'))
    n.require(gas_price > 0 and max_tx > 0 and minimum <= max_tx <= max_block, 'ordinary_gas_order')
    limits = f['limits']
    n.exact(limits, 'max_wire_bytes max_actions max_identifier_bytes max_data_bytes max_memo_bytes max_consensus_key_bytes max_proof_bytes max_expiry_lifetime allowed_algorithms')
    for key in ('max_wire_bytes', 'max_actions', 'max_identifier_bytes', 'max_data_bytes', 'max_memo_bytes', 'max_consensus_key_bytes', 'max_proof_bytes'): positive(limits[key], 2**32 - 1)
    n.require(dec(limits['max_expiry_lifetime']) > 0, 'ordinary_expiry_lifetime')
    algorithms = limits['allowed_algorithms']
    n.require(type(algorithms) is list and algorithms == sorted(set(algorithms)) and 1 <= len(algorithms) <= 2 and set(algorithms) <= {'mldsa65', 'mldsa87'}, 'ordinary_algorithms')
    if network == 1: n.require(algorithms == ['mldsa65'], 'mainnet_accounts_use_mldsa65_only')
    n.require(dec(f['max_block_transaction_bytes']) >= limits['max_wire_bytes'] and dec(f['max_block_signature_checks']) > 0, 'ordinary_block_bounds')
    n.require(set(f['signature_costs']) == set(algorithms) and set(f['validator_proof_costs']) == {'mldsa65'}, 'ordinary_cost_keys')
    n.require(type(f['action_costs']) is list and len(f['action_costs']) == 12, 'ordinary_action_costs')
    n.require(n.amount(f['account_creation_fee_udrt']) > 0, 'ordinary_creation_fee')
    bounds = config.get('governance', {}).get('parameter_bounds', {})
    highest = max(gas_price, bounds['gas_price']['max']) if bounds else gas_price
    # E05-a rule 3: the cap covers the largest signable fee at the highest governed price.
    n.require(n.amount(f['max_fee_cap']) >= max_tx * highest, 'ordinary_fee_cap_below_largest_fee')
    # E05-a rule 5: a full envelope in base64 transport fits the bounds.
    n.require(-(-limits['max_wire_bytes'] // 3) * 4 + TRANSPORT_OVERHEAD <= n.uint(o['max_transport_bytes']) <= config['max_tx_bytes'], 'ordinary_transport_bound')
    n.require(byte_array(f['validator_proof_profile_digest'], 32) == validator_profile_digest(config['lifecycle']), 'validator_proof_profile_digest_mismatch')
    template = o['account_template']; n.exact(template, 'recovery')
    n.require(template['recovery'].get('algorithms') == MLDSA65 and all(n.uint(template['recovery'][k]) > 0 for k in ('timing_version', 'recovery_delay', 'finalization_window', 'policy_delay', 'policy_window', 'submission_lifetime')), 'account_template_invalid')
    n.require(o['initial_grants'] == {} or (type(o['initial_grants']) is dict and len(o['initial_grants']) <= o['max_grants']), 'initial_grants_bound')
    n.require(positive(o['max_state_bytes']) and 1 <= n.uint(o['max_receipts']) <= 65536 and 1 <= n.uint(o['max_retained_profiles']) <= 65536 and positive(o['max_grants']), 'ordinary_state_bounds')
    if 'governance' in config: n.require(o['max_retained_profiles'] >= 2, 'governance_needs_two_profiles')
    for key in ('queue_max_entries', 'queue_max_wire_bytes', 'queue_max_signature_work'): positive(o[key])
    n.require(set(config['lifecycle']['approved_operators'].values()) <= recovery_addresses, 'operator_owner_not_registered')


def governance(config, app_digest, reward):
    g = config['governance']
    n.exact(g, 'schema_version chain_id genesis_digest activation_height fee_profile ballot deposit action_classes parameter_bounds entry_policy')
    n.require(g['schema_version'] == 2 and g['chain_id'] == config['chain_id'] and byte_array(g['genesis_digest'], 32).hex() == app_digest, 'governance_identity_mismatch')
    activation = positive(g['activation_height'])
    v3 = g['fee_profile']; n.exact(v3, 'base version activation_height max_governance_action_bytes governance_action_costs')
    base = config['ordinary']['fee_profile']
    n.require(v3['base'] == base, 'governance_base_differs_from_ordinary_profile')
    n.require(dec(v3['version']) > 0 and dec(v3['activation_height']) == activation and dec(base['activation_height']) <= activation, 'governance_profile_activation')
    action_bytes = g['deposit']['max_action_bytes']
    n.require(0 < n.uint(v3['max_governance_action_bytes']) <= base['limits']['max_wire_bytes'] and v3['max_governance_action_bytes'] == action_bytes, 'governance_action_byte_bound')
    n.require(type(v3['governance_action_costs']) is list and len(v3['governance_action_costs']) == 3 and all(dec(c) > 0 for c in v3['governance_action_costs']), 'governance_action_costs')
    ballot = g['ballot']; n.exact(ballot, 'version chain_id genesis_digest quorum_bps approval_bps veto_bps voting_period_blocks timelock_blocks max_voters')
    n.require(ballot['version'] == 1 and ballot['chain_id'] == config['chain_id'] and ballot['genesis_digest'] == g['genesis_digest'], 'ballot_identity_mismatch')
    # E05-a rule 2: every threshold from 1 to 10,000 basis points.
    n.require(all(1 <= n.uint(ballot[k]) <= 10000 for k in ('quorum_bps', 'approval_bps', 'veto_bps')), 'governance_threshold_bound')
    n.require(positive(ballot['voting_period_blocks']) and positive(ballot['timelock_blocks']) and n.uint(ballot['max_voters']) >= reward['max_positions'], 'ballot_bounds')
    deposit = g['deposit']; n.exact(deposit, 'deposit_period_blocks minimum_deposit_udgt max_action_bytes max_depositors')
    n.require(positive(deposit['deposit_period_blocks']) and 0 < n.uint(deposit['minimum_deposit_udgt'], 10**15) and positive(action_bytes) and positive(deposit['max_depositors']), 'deposit_bounds')
    classes = g['action_classes']
    n.require(type(classes) is list and classes and [c['class'] for c in classes] == sorted({c['class'] for c in classes}) and {c['class'] for c in classes} <= {1, 2}, 'action_classes_invalid')
    for c in classes:
        n.exact(c, 'class max_data_bytes approval_digest')
        n.require(0 < n.uint(c['max_data_bytes']) <= action_bytes and any(byte_array(c['approval_digest'], 32)), 'action_class_bounds')
    n.require(g['entry_policy'] == ENTRY_POLICY, 'entry_policy_differs')
    b = g['parameter_bounds']; n.exact(b, 'gas_price resource_cost account_creation_fee_udrt min_self_bond max_active')
    for name, floor in (('gas_price', 1), ('resource_cost', 0), ('account_creation_fee_udrt', 1), ('min_self_bond', 1), ('max_active', 1)):
        n.exact(b[name], 'min max')
        n.require(floor <= n.uint(b[name]['min'], n.U128_MAX) <= n.uint(b[name]['max'], n.U128_MAX), 'parameter_bound_order')
    n.require(b['max_active']['max'] <= min(64, reward['max_validators']), 'max_active_bound_exceeds_reward_capacity')
    # E05-a rule 4: genesis values lie within their governed bounds.
    inside = lambda value, name: b[name]['min'] <= value <= b[name]['max']
    costs = [base[k] for k in ('transaction_overhead', 'receipt_metadata_cost', 'wire_byte_cost', 'read_byte_cost', 'write_byte_cost')] + base['action_costs'] + list(base['signature_costs'].values()) + list(base['validator_proof_costs'].values()) + v3['governance_action_costs']
    life = config['lifecycle']
    n.require(inside(dec(base['gas_price']), 'gas_price') and inside(n.amount(base['account_creation_fee_udrt']), 'account_creation_fee_udrt') and all(inside(dec(c), 'resource_cost') for c in costs), 'fee_values_outside_governance_bounds')
    n.require(inside(n.amount(life['min_self_bond']), 'min_self_bond') and inside(life['max_active'], 'max_active'), 'lifecycle_values_outside_governance_bounds')


def authority(policy, key):
    a = policy[key]; n.exact(a, 'keys threshold')
    ids = [k.get('key_id') for k in a['keys']]
    n.require(type(a['keys']) is list and a['keys'] and ids == sorted(set(ids)) and all(ids), 'authority_keys_not_sorted_or_unique')
    for k in a['keys']: n.exact(k, 'key_id public_key_hex'); n.require(HEX128.fullmatch(k['public_key_hex']) is not None, 'authority_key_encoding')
    n.require(len({k['public_key_hex'] for k in a['keys']}) == len(a['keys']), 'duplicate_authority_key')
    n.require(1 <= n.uint(a['threshold']) <= len(a['keys']) and a['threshold'] <= policy['max_signatures'], 'authority_threshold')
    return {k['public_key_hex'] for k in a['keys']}, a['threshold']


def control_bound(config, policy, threshold):
    # E05-a rule 6: a control holds its threshold's signatures and fits a transaction.
    n.require(threshold * CONTROL_SIGNATURE_HEX <= n.uint(policy['max_control_bytes']) <= config['max_tx_bytes'], 'root_control_bound')


def root(config, app_digest):
    if not any(k in config for k in ('emergency', 'upgrade', 'release_handover')): return False
    n.require('emergency' in config and ('release_handover' not in config or 'upgrade' in config), 'root_section_dependencies')
    e = config['emergency']
    n.require(set(e) in ({'schema', 'development_only', 'chain_id', 'release_sha512', 'initial_sequence', 'freeze_authority', 'resume_authority', 'max_control_bytes', 'max_signatures', 'automatic_transition_policy'}, {'schema', 'development_only', 'chain_id', 'release_sha512', 'initial_sequence', 'freeze_authority', 'resume_authority', 'max_control_bytes', 'max_signatures', 'automatic_transition_policy', 'v2'}), 'emergency_fields')
    n.require(e['schema'] == (2 if 'v2' in e else 1) and e['chain_id'] == config['chain_id'] and HEX128.fullmatch(e['release_sha512']) is not None and e['automatic_transition_policy'] == 'continue_existing' and n.uint(e['initial_sequence']) >= 1, 'emergency_identity')
    n.require(e['development_only'] is True, 'emergency_development_gate')
    freeze, t1 = authority(e, 'freeze_authority'); resume, t2 = authority(e, 'resume_authority')
    if 'v2' in e:
        v2 = e['v2']; n.exact(v2, 'genesis_sha256 authority_epoch max_validity_blocks max_anchor_age_blocks')
        n.require(v2['genesis_sha256'] == app_digest and all(positive(v2[k]) for k in ('authority_epoch', 'max_validity_blocks', 'max_anchor_age_blocks')), 'emergency_v2_binding')
        n.require(len(freeze) == len(resume) == 5 and t1 == t2 == 3 and not freeze & resume, 'emergency_v2_three_of_five')
    control_bound(config, e, max(t1, t2))
    emergency_keys = freeze | resume
    if 'upgrade' in config:
        u = config['upgrade']
        n.exact(u, 'schema development_only chain_id genesis_sha256 source_release_sha512 authority_epoch authority initial_sequence max_control_bytes max_signatures migration_bounds')
        n.require(u['schema'] == 1 and u['chain_id'] == config['chain_id'] and u['genesis_sha256'] == app_digest and u['source_release_sha512'] == e['release_sha512'] and positive(u['authority_epoch']) and n.uint(u['initial_sequence']) >= 1, 'upgrade_binding')
        n.require(u['development_only'] is True, 'upgrade_development_gate')
        keys, threshold = authority(u, 'authority')
        # Upgrade custodians are a separate group (D11-Q03, P01, 30 September 2026).
        n.require(not keys & emergency_keys, 'upgrade_key_holds_emergency_role')
        control_bound(config, u, threshold)
        m = u['migration_bounds']; n.exact(m, 'max_receipts max_receipt_bytes max_write_bytes')
        for value in m.values(): positive(value)
    if 'release_handover' in config:
        h = config['release_handover']
        n.exact(h, 'schema development_only chain_id genesis_sha256 initial_release_sha512 initial_schema authority_epoch authority initial_sequence max_control_bytes max_signatures')
        n.require(h['schema'] == 1 and h['chain_id'] == config['chain_id'] and h['genesis_sha256'] == app_digest and h['initial_release_sha512'] == config['upgrade']['source_release_sha512'] and h['initial_schema'] == 0 and positive(h['authority_epoch']) and n.uint(h['initial_sequence']) >= 1, 'handover_binding')
        n.require(h['development_only'] is True, 'handover_development_gate')
        _, threshold = authority(h, 'authority')
        control_bound(config, h, threshold)
    return True


def stake(config, genesis, state, recovery_addresses):
    life = config['lifecycle']; reward = genesis['reward_v2']
    for v in config['validators']:
        held = {owner: amount for (owner, validator), amount in state['positions'].items() if validator == v['reward_address']}
        # Voting power is the stake bonded to the validator, in uDGT.
        n.require(v['power'] == sum(held.values()), 'validator_power_differs_from_bonded_stake')
        n.require(held.get(life['approved_operators'][v['reward_address']], 0) >= n.amount(life['min_self_bond']), 'self_bond_below_minimum')
    n.require(life['max_active'] <= reward['max_validators'], 'max_active_exceeds_reward_capacity')
    # Genesis balances go only to registered accounts (account model v2, P01, 26 September 2026).
    n.require(set(state['accounts']) == recovery_addresses, 'native_and_recovery_accounts_differ')
    owners = {owner for owner, _ in state['positions']} | {a for a, acct in state['accounts'].items() if acct['vesting']['kind'] != 'unlocked'}
    n.require(owners <= recovery_addresses, 'reward_owner_not_registered')


def review(config, genesis, native_raw, state):
    """Review a configuration with lifecycle, penalties, recovery and ordinary v2. Returns the sections reviewed."""
    n.require(config['profile'] in (LIFECYCLE_PROFILE, PENALTY_PROFILE) and config['engine'] == 'cometbft-v0.40.0', 'unsupported_consensus_profile')
    n.require(set(config) <= set(BASE_FIELDS) | set(SECTIONS) and set(BASE_FIELDS) <= set(config), 'application_fields')
    n.require(all(k in config for k in ('lifecycle', 'recovery', 'ordinary')), 'extended_profile_needs_lifecycle_recovery_and_ordinary')
    app_digest = hashlib.sha256(native_raw).hexdigest()
    lifecycle(config)
    penalty(config)
    addresses, network = recovery(config, app_digest)
    ordinary(config, network, addresses)
    if 'governance' in config: governance(config, app_digest, genesis['reward_v2'])
    has_root = root(config, app_digest)
    stake(config, genesis, state, addresses)
    return [s for s in SECTIONS if s in config] + (['root_controls'] if has_root else [])


def engine(raw, native_raw, config):
    """Review the engine genesis against the application configuration and native genesis bytes."""
    n.require(raw.endswith(b',"app_state":' + native_raw + b'}'), 'engine_app_state_not_the_exact_native_genesis')
    doc = json.loads(raw, object_pairs_hook=n.unique)
    n.exact(doc, 'genesis_time chain_id initial_height consensus_params validators app_hash app_state')
    n.require(type(doc['genesis_time']) is str and GENESIS_TIME.fullmatch(doc['genesis_time']) is not None, 'engine_genesis_time_not_whole_utc_seconds')
    n.require(doc['chain_id'] == config['chain_id'] and doc['initial_height'] == '1' and doc['app_hash'] == '', 'engine_identity_mismatch')
    p = doc['consensus_params']; n.exact(p, 'block evidence validator version abci authority')
    block_bytes = int(p['block']['max_bytes']); evidence_bytes = int(p['evidence']['max_bytes'])
    n.require(0 < block_bytes <= 104_857_600 and int(p['block']['max_gas']) >= -1 and 0 <= evidence_bytes <= block_bytes, 'engine_block_bounds')
    n.require(p['validator'] == {'pub_key_types': ['ml_dsa_65']} and p['abci'] == {'vote_extensions_enable_height': '0'}, 'engine_key_or_extension_profile')
    if 'lifecycle' in config:
        life = config['lifecycle']
        n.require(p['evidence']['max_age_num_blocks'] == str(life['evidence_max_age_blocks']) and p['evidence']['max_age_duration'] == str(life['evidence_max_age_seconds'] * 10**9), 'engine_evidence_differs_from_lifecycle')
    engine_set = {}
    for v in doc['validators']:
        n.exact(v, 'address pub_key power name'); n.exact(v['pub_key'], 'type value')
        key = base64.b64decode(v['pub_key']['value'], validate=True)
        n.require(v['pub_key']['type'] == 'cometbft/PubKeyMlDsa65' and len(key) == 1952 and v['address'] == hashlib.sha256(key).hexdigest()[:40].upper(), 'engine_validator_key_or_address')
        n.require(v['pub_key']['value'] not in engine_set, 'engine_duplicate_validator')
        engine_set[v['pub_key']['value']] = int(v['power'])
    n.require(engine_set == {v['pubkey_base64']: v['power'] for v in config['validators']}, 'engine_validators_differ_from_application')
    return doc


def manifest(document, files):
    """Check a builder manifest against the supplied file bytes."""
    n.require(document.get('schema') == 'dytallix.genesis-build-manifest.v1' and document.get('production') is False, 'manifest_schema_or_production_flag')
    listed = document['files']
    n.require(set(listed) == set(files), 'manifest_file_set')
    build = hashlib.sha256(b'DYTALLIX/GENESIS-BUILD/v1\0')
    for name in ('native-genesis.json', 'application-config.json', 'genesis.json'):
        raw = files[name]; entry = listed[name]
        n.require(entry == {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest(), 'sha512': hashlib.sha512(raw).hexdigest()}, 'manifest_file_digest_mismatch')
        build.update(len(raw).to_bytes(8, 'big') + raw)
    n.require(document['build_digest'] == build.hexdigest(), 'manifest_build_digest_mismatch')
