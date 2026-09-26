"""Synthetic local fixtures exercise typed bindings. No production input is selected."""
import base64
import copy
import json
import os
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
import check_bindings as c

FIXTURES=Path(os.environ.get('DYTX_BINDING_FIXTURES',Path(__file__).parent/'fixtures'))

class BindingTests(unittest.TestCase):
    def setUp(self):
        self.b=c.decode((FIXTURES/'bindings.json').read_bytes());self.r=c.decode((FIXTURES/'records.json').read_bytes());self.g=c.decode((FIXTURES/'native.json').read_bytes());self.a=c.decode((FIXTURES/'application.json').read_bytes())
    def result(self,sync=False):
        nr=c.n.canonical(self.g);self.a['app_state_sha256']=c.digest(nr);ar=c.n.canonical(self.a);rr=c.n.canonical(self.r)
        self.b['source_digests']={'records_sha256':c.digest(rr),'native_genesis_sha256':c.digest(nr),'application_config_sha256':c.digest(ar)}
        if sync:
            for field,value in [('consensus_configuration',self.a),('reward_parameters',self.g['reward_v2']),('issuance_parameters',self.g['adaptive_issuance'])]:self.b['runtime_inputs'][field]=copy.deepcopy(value)
        return c.validate(self.b,self.r,rr,nr,ar)
    def rejected(self,change):
        change();errors=self.result()['errors'];self.assertTrue(any(not error['scope'].endswith('_exact_source_binding') for error in errors),errors)
    def doc(self,kind):return next(x for x in self.b['public_documents'] if x['document']['kind']==kind)
    def positive_bootstrap(self):
        binding=self.b['runtime_inputs']['account_bindings'][0];record=self.r['records']['D08-Q01']['rows'][binding['allocation_row']]
        next(a for a in self.g['accounts'] if a['address']==binding['address'])['balances']['udrt']='5'
        binding['bootstrap_rows']=[0];self.r['policy_inputs']['drt_bootstrap']['amount_base_units']='5'
        self.r['records']['D08-Q03']['rows']=[{'recipient_account_reference':record['recipient_account_reference'],'amount_base_units':'5','approved_bootstrap_policy_reference':'fixture-bootstrap-policy','custody_reference':None,'approval_evidence':None}]
    def test_positive_drt_binding(self):
        self.positive_bootstrap();self.assertFalse(self.result(sync=True)['errors'])
    def test_drt_wrong_amount(self):
        self.positive_bootstrap();self.r['records']['D08-Q03']['rows'][0]['amount_base_units']='6'
        self.assertTrue(any(e['scope']=='beneficiary_amount_vesting_stake_bindings' for e in self.result(sync=True)['errors']))
    def test_drt_wrong_recipient(self):
        self.positive_bootstrap();self.r['records']['D08-Q03']['rows'][0]['recipient_account_reference']=self.r['records']['D08-Q01']['rows'][1]['recipient_account_reference']
        self.assertTrue(any(e['scope']=='beneficiary_amount_vesting_stake_bindings' for e in self.result(sync=True)['errors']))
    def test_drt_wrong_policy(self):
        self.positive_bootstrap();self.r['records']['D08-Q03']['rows'][0]['approved_bootstrap_policy_reference']='other'
        self.assertTrue(any(e['scope']=='beneficiary_amount_vesting_stake_bindings' for e in self.result(sync=True)['errors']))
    def test_duplicate_drt_row_binding(self):
        self.positive_bootstrap();self.b['runtime_inputs']['account_bindings'][0]['bootstrap_rows']=[0,0]
        self.assertTrue(any(e['scope']=='beneficiary_amount_vesting_stake_bindings' for e in self.result(sync=True)['errors']))
    def malformed_runtime(self,genesis,application):
        nr=c.n.canonical(genesis);ar=c.n.canonical(application);rr=c.n.canonical(self.r)
        self.b['source_digests']={'records_sha256':c.digest(rr),'native_genesis_sha256':c.digest(nr),'application_config_sha256':c.digest(ar)}
        return c.validate(self.b,self.r,rr,nr,ar)
    def test_nonobject_runtime_cli_rejected_without_traceback(self):
        for location in ('native','application'):
            for bad in ([],None,7):
                with self.subTest(location=location,bad=bad),tempfile.TemporaryDirectory() as temp:
                    root=Path(temp);values={'native':self.g,'application':self.a,'records':self.r,'bindings':self.b};values[location]=bad
                    for key,value in values.items():(root/(key+'.json')).write_bytes(c.n.canonical(value))
                    command=[sys.executable,'-B',str(Path(c.__file__).resolve())]
                    for key in ('bindings','records','native','application'):command+=['--'+key,str(root/(key+'.json'))]
                    result=subprocess.run(command,capture_output=True,text=True,timeout=10)
                    self.assertEqual(result.returncode,2);self.assertFalse(result.stderr);self.assertTrue(json.loads(result.stdout)['errors'])
    def test_nonobject_runtime_report_rejected(self):
        for bad in ([],None,7):
            with self.subTest(native=bad):self.assertTrue(self.malformed_runtime(bad,self.a)['errors'])
            with self.subTest(application=bad):self.assertTrue(self.malformed_runtime(self.g,bad)['errors'])
    def test_exact_application_byte_limit(self):
        nr=c.n.canonical(self.g);self.a['app_state_sha256']=c.digest(nr);ar=c.n.canonical(self.a);ar+=b' '*(65537-len(ar));rr=c.n.canonical(self.r)
        self.b['source_digests']={'records_sha256':c.digest(rr),'native_genesis_sha256':c.digest(nr),'application_config_sha256':c.digest(ar)}
        result=c.validate(self.b,self.r,rr,nr,ar)
        self.assertTrue(any(e['code']=='application_input_byte_bound' for e in result['errors']))
    def test_scoped_ipv6_loopback_rejected(self):
        self.rejected(lambda:self.b['runtime_inputs']['network_configuration']['transport']['peers'][0].update(address='[::1%lo0]:26656'))
    def test_supported_local_values_validate_without_activation(self):
        r=self.result();self.assertFalse(r['errors']);self.assertFalse(r['runtime_complete']);self.assertFalse(r['production_accepted']);self.assertEqual(r['status'],'BLOCKED');self.assertGreaterEqual(len(r['checks']),8)
    def test_populated_root_does_not_count_complete(self):
        self.b['runtime_inputs']['root_authorization']={'reference':'present'};r=self.result()
        self.assertFalse(r['runtime_complete']);self.assertTrue(any(x['field']=='root_authorization' and x['supplied'] for x in r['unsupported']))
    def test_populated_governance_does_not_count_complete(self):
        self.b['runtime_inputs']['governance_parameters']={'voting':'bonded'};r=self.result();self.assertTrue(any(x['field']=='governance_parameters' and x['supplied'] for x in r['unsupported']))
    def test_missing_inputs_stay_missing(self):
        for key in self.b['runtime_inputs']:self.b['runtime_inputs'][key]=None
        r=self.result();self.assertEqual(len(r['missing']),11);self.assertFalse(r['runtime_complete'])
    def test_records_hash_mismatch(self):
        rr=c.n.canonical(self.r);self.b['source_digests']['records_sha256']='0'*64
        self.assertTrue(c.validate(self.b,self.r,rr)['errors'])
    def test_public_document_hash_mismatch(self):self.rejected(lambda:self.doc('account')['document'].update(address='other'))
    def test_private_key_fields_rejected(self):
        row=self.doc('validator_key');row['document']['private_key']='forbidden';row['sha256']=c.digest(c.n.canonical(row['document']));self.assertTrue(self.result()['errors'])
    def test_duplicate_public_reference(self):self.rejected(lambda:self.b['public_documents'].append(copy.deepcopy(self.b['public_documents'][0])))
    def test_unresolved_account_reference(self):self.rejected(lambda:self.r['records']['D08-Q01']['rows'][0].update(recipient_account_reference='missing'))
    def test_wrong_beneficiary_amount(self):self.rejected(lambda:self.r['records']['D08-Q01']['rows'][0].update(amount_base_units='1'))
    def test_wrong_initial_mint_policy(self):self.rejected(lambda:self.r['policy_inputs']['dgt_initial_supply'].update(amount_base_units='1'))
    def test_missing_staking_permission(self):self.rejected(lambda:self.r['records']['D08-Q01']['rows'][0].update(staking_permission=None))
    def test_unmapped_allocation_row(self):self.rejected(lambda:self.r['records']['D08-Q01']['rows'].append(copy.deepcopy(self.r['records']['D08-Q01']['rows'][0])))
    def test_duplicate_account_binding(self):self.rejected(lambda:self.b['runtime_inputs']['account_bindings'].append(copy.deepcopy(self.b['runtime_inputs']['account_bindings'][0])))
    def test_duplicate_validator_binding(self):self.rejected(lambda:self.b['runtime_inputs']['validator_bindings'].append(copy.deepcopy(self.b['runtime_inputs']['validator_bindings'][0])))
    def test_wrong_operator_key(self):self.rejected(lambda:self.r['records']['D09-Q02']['rows'][0].update(public_key_reference=self.r['records']['D09-Q02']['rows'][1]['public_key_reference']))
    def test_unknown_delegated_operator(self):
        row=next(x for x in self.r['records']['D08-Q01']['rows'] if x['initial_delegation']['kind']=='delegated');row['initial_delegation']['validator_operator_id']='absent';self.assertTrue(self.result()['errors'])
    def test_wrong_delegation_amount(self):
        row=next(x for x in self.r['records']['D08-Q01']['rows'] if x['initial_delegation']['kind']=='delegated');row['initial_delegation']['amount_base_units']='26';self.assertTrue(self.result()['errors'])
    def test_unmatched_vesting_terms(self):
        row=self.doc('vesting_terms');row['document']['vesting']={'kind':'linear_after_cliff','total_amount':'1','start_time':0,'cliff_duration':0,'vesting_duration':1,'allow_staking':False};row['sha256']=c.digest(c.n.canonical(row['document']));self.assertTrue(self.result()['errors'])
    def test_production_consensus_profile_not_accepted(self):self.rejected(lambda:self.a.update(profile='production'))
    def test_extended_application_profile_stays_unsupported(self):
        self.a['ordinary']={'present':True};r=self.result();self.assertTrue(any(x['field']=='extended_application_profile' for x in r['unsupported']));self.assertFalse(r['runtime_complete'])
    def test_consensus_resource_bounds(self):self.rejected(lambda:self.a.update(max_tx_bytes=262145))
    def test_validator_boolean_power_rejected(self):self.rejected(lambda:self.a['validators'][0].update(power=True))
    def test_classical_key_type_rejected(self):self.rejected(lambda:self.a['validators'][0].update(pubkey_type='ed25519'))
    def test_duplicate_validator_key(self):self.rejected(lambda:self.a['validators'][0].update(pubkey_base64=self.a['validators'][1]['pubkey_base64']))
    def test_power_total_bound(self):self.rejected(lambda:self.a['validators'][0].update(power=2**63-1))
    def test_reward_set_mismatch(self):self.rejected(lambda:self.g['reward_v2']['validators'].pop())
    def test_locked_owner_resource_limit(self):self.rejected(lambda:self.g['reward_v2'].update(max_positions=1))
    def test_invalid_timing_budget(self):self.rejected(lambda:self.g['adaptive_issuance'].update(initial_epoch_budget_udrt='1001'))
    def test_runtime_reward_boolean_alias_rejected(self):
        self.b['runtime_inputs']['reward_parameters']['activation_height']=True
        self.assertTrue(any(e['scope']=='reward_parameters_exact_source_binding' for e in self.result()['errors']))
    def test_duplicate_native_account(self):self.rejected(lambda:self.g['accounts'].append(copy.deepcopy(self.g['accounts'][0])))
    def test_native_u128_overflow(self):self.rejected(lambda:self.g['accounts'][0]['balances'].update(udrt=str(2**128)))
    def test_native_supply_cap(self):self.rejected(lambda:self.g['accounts'][0]['balances'].update(udgt=str(10**15+1)))
    def test_funding_positions_mismatch(self):self.rejected(lambda:self.g['staking']['delegations'][0].update(amount_udgt='26'))
    def test_unknown_native_fields(self):self.rejected(lambda:self.g.update(unrecognized=True))
    def test_chain_record_mismatch(self):self.rejected(lambda:self.b['runtime_inputs']['chain_id'].update(value='other'))
    def test_transport_peer_id_mismatch(self):self.rejected(lambda:self.b['runtime_inputs']['network_configuration']['transport']['peers'][0].update(id='0'*40))
    def test_nonlocal_transport_rejected(self):self.rejected(lambda:self.b['runtime_inputs']['network_configuration']['transport']['peers'][0].update(address='192.0.2.1:26656'))
    def test_peer_persistent_binding_mismatch(self):self.rejected(lambda:self.b['runtime_inputs']['network_configuration'].update(persistent_peers=[]))
    def test_handshake_timeout_bound(self):self.rejected(lambda:self.b['runtime_inputs']['network_configuration']['transport'].update(handshake_timeout_ms=10001))
    def test_json_duplicate_key(self):
        with self.assertRaises(ValueError):c.decode(b'{"version":1,"version":2}')
    def test_json_nonfinite(self):
        with self.assertRaises(ValueError):c.decode(b'{"version":NaN}')
    def test_json_depth_bound(self):
        with self.assertRaises(ValueError):c.decode(('['*65+'0'+']'*65).encode())
    def test_row_boolean_index_rejected(self):self.rejected(lambda:self.b['runtime_inputs']['account_bindings'][0].update(allocation_row=True))

if __name__=='__main__':unittest.main(verbosity=2)
