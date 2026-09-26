#!/usr/bin/env python3
"""Validate supplied public runtime bindings. Emit review evidence, never genesis."""
import argparse
import base64
import hashlib
import ipaddress
import json
from pathlib import Path
import re
import unicodedata
import native_checks as n

LIMIT = 8*1024*1024
RUNTIME_FIELDS = 'chain_id genesis_time account_bindings validator_bindings governance_parameters issuance_parameters reward_parameters consensus_configuration network_configuration root_authorization approval_bundle'.split()
SUPPORTED = {'chain_id','account_bindings','validator_bindings','issuance_parameters','reward_parameters','consensus_configuration','network_configuration'}


def decode(raw):
    n.require(len(raw)<=LIMIT,'input_byte_limit')
    try: value=json.loads(raw,object_pairs_hook=n.unique,parse_constant=lambda _:(_ for _ in ()).throw(ValueError('nonfinite_json')))
    except RecursionError as exc: raise ValueError('json_depth_bound') from exc
    pending=[(value,0)]
    while pending:
        item,depth=pending.pop();n.require(depth<=64,'json_depth_bound')
        if isinstance(item,dict):pending.extend((v,depth+1) for v in item.values())
        elif isinstance(item,list):pending.extend((v,depth+1) for v in item)
    return value

def read(path):
    with Path(path).open('rb') as stream: raw=stream.read(LIMIT+1)
    return raw,decode(raw)


def digest(raw): return hashlib.sha256(raw).hexdigest()


def ident(value):
    n.require(type(value) is str and 0<len(value.encode('utf-8'))<=256 and
              not any(c.isspace() or unicodedata.category(c)=='Cc' for c in value),'invalid_identity')
    return value


def pubkey(value):
    n.require(type(value) is str,'public_key_must_be_string')
    try: raw=base64.b64decode(value,validate=True)
    except (ValueError,TypeError) as exc: raise ValueError('invalid_public_key_encoding') from exc
    n.require(len(raw)==1952 and base64.b64encode(raw).decode()==value,'invalid_mldsa65_public_key')
    return raw


def native(genesis):
    n.exact(genesis,'chain_id accounts staking reward_v2 adaptive_issuance')
    ident(genesis['chain_id']); accounts={}; dgt=drt=0; locks=set()
    n.require(type(genesis['accounts']) is list and len(genesis['accounts'])<=10000,'account_review_bound')
    for a in genesis['accounts']:
        n.exact(a,'address balances vesting'); ident(a['address']);n.exact(a['balances'],'udgt udrt')
        n.require(a['address'] not in accounts,'duplicate_account')
        g=n.amount(a['balances']['udgt']);r=n.amount(a['balances']['udrt']);dgt+=g;drt+=r
        n.vesting(a['vesting'],g)
        if a['vesting']['kind']!='unlocked':locks.add(a['address'])
        accounts[a['address']]=a
    n.require(dgt<=10**15 and drt<=n.U128_MAX,'native_supply_bound')
    n.exact(genesis['staking'],'delegations'); stakes={}
    for s in genesis['staking']['delegations']:
        n.exact(s,'delegator amount_udgt');owner=s['delegator'];value=n.amount(s['amount_udgt'])
        n.require(owner in accounts and owner not in stakes,'invalid_or_duplicate_delegator')
        n.require(value<=n.amount(accounts[owner]['balances']['udgt']),'unfunded_stake');stakes[owner]=value
    reward=genesis['reward_v2'];n.exact(reward,'version activation_height decimals profile max_validators max_positions validators positions')
    n.require(type(reward['version']) is int and reward['version']==2 and type(reward['decimals']) is int and reward['decimals']==6 and type(reward['activation_height']) is int and reward['activation_height']==1 and reward['profile']=='development','unsupported_reward_profile')
    n.require(1<=n.uint(reward['max_validators'])<=10000 and 1<=n.uint(reward['max_positions'])<=10000,'reward_resource_bound')
    validators=set()
    for v in reward['validators']:
        n.exact(v,'address active jailed');ident(v['address'])
        n.require(v['address'] not in validators and v['active'] is True and v['jailed'] is False,'reward_validator_invalid')
        validators.add(v['address'])
    totals={};positions={}
    for p in reward['positions']:
        n.exact(p,'owner validator amount_udgt');owner=p['owner'];validator=p['validator'];value=n.amount(p['amount_udgt'])
        n.require(owner in accounts and validator in validators and value>0 and (owner,validator) not in positions,'invalid_or_duplicate_position')
        if owner in locks:n.require(accounts[owner]['vesting']['allow_staking'],'locked_staking_not_permitted')
        positions[(owner,validator)]=value;totals[owner]=totals.get(owner,0)+value
    n.require(totals==stakes,'funded_stake_position_mismatch')
    n.require(len(validators)<=reward['max_validators'] and len(positions)<=reward['max_positions'] and len(locks|set(totals))<=reward['max_positions'],'reward_population_bound')
    n.validate_timing(genesis['adaptive_issuance'])
    return {'accounts':accounts,'dgt':dgt,'drt':drt,'positions':positions,'validators':validators}


def application(config,genesis,raw):
    n.exact(config,'profile engine chain_id app_state_sha256 gas_price max_tx_bytes max_block_bytes max_txs validators')
    n.require(config['profile']=='cometbft-local-qualification' and config['engine']=='cometbft-v0.40.0','unsupported_consensus_profile')
    n.require(config['chain_id']==genesis['chain_id'] and len(config['chain_id'].encode())<=50,'application_chain_mismatch')
    n.require(config['app_state_sha256']==digest(raw),'application_genesis_digest_mismatch')
    n.require(0<n.uint(config['gas_price'])<=2**63-1,'gas_price_bound')
    n.require(1<=n.uint(config['max_tx_bytes'])<=262144 and 1<=n.uint(config['max_block_bytes'])<=1048576 and config['max_tx_bytes']<=config['max_block_bytes'] and 1<=n.uint(config['max_txs'])<=1000,'consensus_resource_bound')
    n.require(type(config['validators']) is list and 1<=len(config['validators'])<=1000,'validator_count_bound')
    keys=set();addresses=set();power=0
    for v in config['validators']:
        n.exact(v,'pubkey_type pubkey_base64 power reward_address');ident(v['reward_address'])
        key=pubkey(v['pubkey_base64']);n.require(v['pubkey_type']=='ml_dsa_65','unsupported_validator_algorithm')
        n.require(key not in keys and v['reward_address'] not in addresses,'duplicate_validator')
        n.require(type(v['power']) is int and 0<v['power']<=2**63-1,'validator_power_bound')
        keys.add(key);addresses.add(v['reward_address']);power+=v['power']
    n.require(power<=(2**63-1)//8 and len(n.canonical(config))<=65536,'application_storage_bound')
    n.require(addresses=={v['address'] for v in genesis['reward_v2']['validators']},'reward_consensus_set_mismatch')
    return {v['reward_address']:v for v in config['validators']}


def documents(rows):
    n.require(type(rows) is list and len(rows)<=10000,'document_review_bound');out={}
    for row in rows:
        n.exact(row,'reference sha256 document');ref=ident(row['reference']);doc=row['document']
        n.require(ref not in out and row['sha256']==digest(n.canonical(doc)),'duplicate_or_mismatched_public_document')
        n.require(type(doc) is dict,'document_must_be_object')
        kind=doc.get('kind')
        fields={'account':'kind address','validator_key':'kind algorithm public_key_base64','peer_key':'kind algorithm public_key_base64','vesting_terms':'kind vesting','identity_policy':'kind chain_id'}
        n.require(kind in fields,'unsupported_public_document_kind');n.exact(doc,fields[kind])
        if kind=='account':ident(doc['address'])
        if kind=='identity_policy':ident(doc['chain_id'])
        if kind in ('validator_key','peer_key'):
            n.require(doc['algorithm']=='ML-DSA-65','unsupported_document_algorithm');pubkey(doc['public_key_base64'])
        if kind=='vesting_terms':
            n.require(type(doc['vesting']) is dict,'vesting_document_type')
            if doc['vesting'].get('kind')=='unlocked':n.exact(doc['vesting'],'kind')
            else:n.vesting(doc['vesting'],n.amount(doc['vesting'].get('total_amount')))
        out[ref]=doc
    return out


def row_at(records,record_id,index):
    n.require(type(index) is int and index>=0,'record_index_type')
    rows=records['records'][record_id]['rows'];n.require(index<len(rows),'record_index_missing')
    return rows[index]


def doc_at(docs,reference,kind):
    n.require(type(reference) is str and reference in docs and docs[reference]['kind']==kind,'public_reference_unresolved')
    return docs[reference]


def validators_binding(values,records,docs,validators):
    n.require(type(values) is list,'validator_binding_array');seen=set();operators={};used=set()
    for binding in values:
        n.exact(binding,'reward_address record_row');address=binding['reward_address'];index=binding['record_row']
        n.require(address in validators and address not in seen and index not in used,'validator_binding_coverage')
        record=row_at(records,'D09-Q02',index);operator=ident(record['operator_id'])
        n.require(operator not in operators,'duplicate_operator_binding')
        key=doc_at(docs,record['public_key_reference'],'validator_key')
        n.require(key['public_key_base64']==validators[address]['pubkey_base64'],'operator_public_key_mismatch')
        operators[operator]=address;seen.add(address);used.add(index)
    n.require(seen==set(validators),'validator_binding_coverage')
    return operators


def accounts_binding(values,records,docs,state,operators):
    n.require(type(values) is list,'account_binding_array');seen=set();used=set();drt_used=set()
    for binding in values:
        n.exact(binding,'address allocation_row bootstrap_rows');address=binding['address'];index=binding['allocation_row']
        n.require(address in state['accounts'] and address not in seen and index not in used,'account_binding_coverage')
        record=row_at(records,'D08-Q01',index);account=state['accounts'][address]
        n.require(doc_at(docs,record['recipient_account_reference'],'account')['address']==address,'recipient_account_mismatch')
        n.require(n.amount(record['amount_base_units'])==n.amount(account['balances']['udgt']),'allocation_native_amount_mismatch')
        terms=record['explicit_vesting_schedule'];n.exact(terms,'kind approved_terms_reference')
        vesting=doc_at(docs,terms['approved_terms_reference'],'vesting_terms')['vesting']
        n.require(vesting==account['vesting'],'vesting_terms_mismatch')
        n.require(terms['kind']==('none' if vesting['kind']=='unlocked' else 'explicit_schedule'),'vesting_kind_mismatch')
        permission=record['staking_permission'];n.require(type(permission) is bool,'staking_permission_missing')
        if vesting['kind']!='unlocked':n.require(permission==vesting['allow_staking'],'staking_permission_mismatch')
        initial=record['initial_delegation'];positions={v:amount for (owner,v),amount in state['positions'].items() if owner==address}
        n.require(type(initial) is dict,'delegation_input_missing')
        if initial.get('kind')=='none':n.exact(initial,'kind');n.require(not positions,'unexpected_genesis_stake')
        else:
            n.exact(initial,'kind validator_operator_id amount_base_units');n.require(initial['kind']=='delegated' and permission,'delegation_not_permitted')
            n.require(initial['validator_operator_id'] in operators,'delegation_operator_missing')
            n.require(positions=={operators[initial['validator_operator_id']]:n.amount(initial['amount_base_units'])},'delegation_binding_mismatch')
        total=0;n.require(type(binding['bootstrap_rows']) is list,'bootstrap_binding_array')
        for drt_index in binding['bootstrap_rows']:
            n.require(type(drt_index) is int and drt_index not in drt_used,'duplicate_bootstrap_binding')
            drt=row_at(records,'D08-Q03',drt_index);drt_used.add(drt_index)
            n.require(doc_at(docs,drt['recipient_account_reference'],'account')['address']==address,'bootstrap_recipient_mismatch')
            n.require(type(drt['approved_bootstrap_policy_reference']) is str and drt['approved_bootstrap_policy_reference']==records['policy_inputs']['drt_bootstrap']['policy_approval_reference'],'bootstrap_policy_mismatch')
            total+=n.amount(drt['amount_base_units'])
        n.require(total==n.amount(account['balances']['udrt']),'bootstrap_native_amount_mismatch')
        seen.add(address);used.add(index)
    n.require(seen==set(state['accounts']),'account_binding_coverage')
    n.require(used==set(range(len(records['records']['D08-Q01']['rows']))),'unmapped_allocation_records')
    n.require(drt_used==set(range(len(records['records']['D08-Q03']['rows']))),'unmapped_bootstrap_records')
    n.require(n.amount(records['policy_inputs']['dgt_initial_supply']['amount_base_units'])==state['dgt'] and n.amount(records['policy_inputs']['drt_bootstrap']['amount_base_units'])==state['drt'],'initial_supply_policy_mismatch')


def transport_binding(value,docs,chain):
    n.exact(value,'transport persistent_peers local_peer_reference');t=value['transport']
    n.exact(t,'version profile network local_public_key_base64 peers handshake_timeout_ms')
    n.require(type(t['version']) is int and t['version']==1 and t['profile']=='dytallix-pqc-loopback-v1' and t['network']==chain,'unsupported_or_mismatched_transport')
    local=pubkey(t['local_public_key_base64']);n.require(doc_at(docs,value['local_peer_reference'],'peer_key')['public_key_base64']==t['local_public_key_base64'],'local_public_pin_mismatch')
    n.require(100<=n.uint(t['handshake_timeout_ms'],2**32-1)<=10000 and type(t['peers']) is list and 1<=len(t['peers'])<=64,'transport_resource_bound')
    seen=set();addresses=set();persistent=set()
    for peer in t['peers']:
        n.exact(peer,'id public_key_base64 address');key=pubkey(peer['public_key_base64']);peer_id=digest(key)[:40]
        n.require(peer['id']==peer_id and key!=local and peer_id not in seen,'peer_id_key_mismatch')
        address=peer['address'];n.require(type(address) is str,'peer_address_type')
        match=re.fullmatch(r'(?:\[([^]]+)\]|([^:]+)):([0-9]+)',address);n.require(match is not None,'peer_address_encoding')
        host=match.group(1) or match.group(2);n.require('%' not in host,'scoped_peer_address_not_supported')
        ip=ipaddress.ip_address(host);n.require(ip.is_loopback and 1<=int(match.group(3))<=65535 and address not in addresses,'nonlocal_or_duplicate_peer')
        seen.add(peer_id);addresses.add(address);persistent.add(peer_id+'@'+address)
    n.require(type(value['persistent_peers']) is list and len(value['persistent_peers'])==len(persistent) and set(value['persistent_peers'])==persistent,'persistent_peer_binding_mismatch')
    return len(seen)


def validate(bindings,records,records_raw,native_raw=None,config_raw=None):
    result={'status':'BLOCKED','production_accepted':False,'runtime_complete':False,'genesis_emitted':False,'activation_enabled':False,'checks':[],'errors':[],'missing':[],'unsupported':[]}
    def check(label,fn):
        try:value=fn();result['checks'].append(label);return value
        except (ValueError,TypeError,KeyError,IndexError,UnicodeError) as exc:result['errors'].append({'scope':label,'code':str(exc) if isinstance(exc,ValueError) else 'invalid_field_type_or_shape'});return None
    try:
        n.require(type(records) is dict and type(records.get('records')) is dict and type(records.get('policy_inputs')) is dict,'record_packet_must_be_object')
        n.exact(bindings,'schema_version profile source_digests runtime_inputs public_documents')
        n.require(type(bindings['schema_version']) is int and bindings['schema_version']==1 and bindings['profile']=='production-runtime-review-only','unsupported_binding_version_or_profile')
        n.exact(bindings['runtime_inputs'],' '.join(RUNTIME_FIELDS));n.exact(bindings['source_digests'],'records_sha256 native_genesis_sha256 application_config_sha256')
        n.require(bindings['source_digests']['records_sha256']==digest(records_raw),'records_digest_mismatch')
    except (ValueError,TypeError,KeyError) as exc:result['errors'].append({'scope':'binding_contract','code':str(exc)});return result
    runtime=bindings['runtime_inputs']
    for name in RUNTIME_FIELDS:
        if runtime[name] is None:result['missing'].append(name)
        if name not in SUPPORTED:result['unsupported'].append({'field':name,'supplied':runtime[name] is not None,'reason':'No implemented typed consumer adapter'})
    result['unsupported'] += [{'field':x,'reason':'Not established by public structural review'} for x in ['production_activation','custody_and_approval_authenticity','validator_admission_and_power_policy','engine_toml_isolation_and_private_key_binding']]
    docs=check('public_document_hashes_and_types',lambda:documents(bindings['public_documents']))
    if native_raw is None or config_raw is None:
        result['missing'].append('native_genesis_or_application_bytes');return result
    try:
        n.require(len(config_raw)<=65536,'application_input_byte_bound')
        genesis=decode(native_raw);config=decode(config_raw)
        n.require(type(genesis) is dict and type(config) is dict,'runtime_document_must_be_object')
        n.require(bindings['source_digests']['native_genesis_sha256']==digest(native_raw) and bindings['source_digests']['application_config_sha256']==digest(config_raw),'runtime_source_digest_mismatch')
    except (ValueError,TypeError,KeyError) as exc:result['errors'].append({'scope':'runtime_source','code':str(exc)});return result
    if any(key in config for key in ('lifecycle','penalty','recovery','ordinary')):
        result['unsupported'].append({'field':'extended_application_profile','reason':'Typed adapter not implemented for lifecycle, penalty, recovery or ordinary config'});return result
    state=check('native_monetary_reward_timing',lambda:native(genesis))
    validators=check('application_configuration',lambda:application(config,genesis,native_raw))
    for name,expected in [('reward_parameters',genesis.get('reward_v2')),('issuance_parameters',genesis.get('adaptive_issuance')),('consensus_configuration',config)]:
        if runtime[name] is not None:check(name+'_exact_source_binding',lambda name=name,expected=expected:n.require(n.canonical(runtime[name])==n.canonical(expected),'runtime_parameter_source_mismatch'))
    if docs is not None and runtime['chain_id'] is not None:
        def identity_check():
            v=runtime['chain_id'];n.exact(v,'value identity_record_row');row=row_at(records,'D13-Q02',v['identity_record_row']);policy=doc_at(docs,row['approved_identity_policy_reference'],'identity_policy')
            n.require(v['value']==policy['chain_id']==genesis['chain_id']==config['chain_id'],'identity_chain_mismatch')
        check('chain_identity_record_binding',identity_check)
    operators=None
    if validators is not None and docs is not None and runtime['validator_bindings'] is not None:operators=check('operator_key_bindings',lambda:validators_binding(runtime['validator_bindings'],records,docs,validators))
    if state is not None and docs is not None and operators is not None and runtime['account_bindings'] is not None:check('beneficiary_amount_vesting_stake_bindings',lambda:accounts_binding(runtime['account_bindings'],records,docs,state,operators))
    if docs is not None and runtime['network_configuration'] is not None:check('public_transport_pin_bindings',lambda:transport_binding(runtime['network_configuration'],docs,genesis['chain_id']))
    result['supported_supplied_fields_valid']=not result['errors']
    result['limits']=['Public reference and hash equality is not approval or signature verification.','Current profiles are local development only. Production startup remains disabled.','Records schema and acceptance require the separate intake checker.','Allocation-entitlement versus partial genesis mint needs a separate approved adapter; this version requires exact amounts.']
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--bindings',type=Path,required=True);parser.add_argument('--records',type=Path,required=True);parser.add_argument('--native',type=Path);parser.add_argument('--application',type=Path);args=parser.parse_args()
    try:
        _,b=read(args.bindings);rr,r=read(args.records);nr=read(args.native)[0] if args.native else None;cr=read(args.application)[0] if args.application else None
        result=validate(b,r,rr,nr,cr)
    except (OSError,ValueError,TypeError,KeyError):result={'status':'BLOCKED','errors':[{'scope':'input','code':'input_unreadable_or_invalid'}],'production_accepted':False,'runtime_complete':False,'genesis_emitted':False}
    print(json.dumps(result,indent=2));return 2 if result['errors'] else 1

if __name__=='__main__':raise SystemExit(main())
