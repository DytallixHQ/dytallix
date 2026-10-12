"""The reconciliation report (simulation plan, T6): its readings, its rules,
what makes it PASS, FAIL or INCONCLUSIVE, its digest-bound evidence, and the
collector against stand-in hosts and a stand-in client. The real run is the
Host install workflow's Reconciliation step."""
import copy
import json
import unittest

import reconcile as rc

GENESIS = {'accounts': [{'address': 'a', 'balances': {'udgt': '600', 'udrt': '1000'}},
                        {'address': 'b', 'balances': {'udgt': '400', 'udrt': '0'}}]}
GENESIS_BYTES = json.dumps(GENESIS).encode()
TOTALS = rc.genesis_totals(GENESIS)


def supply(emitted=50, burned=20, pool=30, liquid_dgt=900, staked=100):
    total = 1000 + emitted - burned
    return {'udrt': {'genesis': '1000', 'emitted': str(emitted), 'burned': str(burned), 'total': str(total),
                     'liquid': str(total - pool), 'withheld_fees': '0', 'pool_staking': str(pool)},
            'udgt': {'issued': '1000', 'liquid': str(liquid_dgt), 'staked': str(staked), 'pending_bonded': '0',
                     'unbonding': '0', 'penalty_reserve': '0', 'governance_escrow': '0'}}


def prom(height, s):
    lines = [f'dytallix_app_height {height}']
    for denomination, buckets in s.items():
        lines += [f'dytallix_app_supply_{denomination}{{bucket="{b}"}} {v}' for b, v in buckets.items()]
    return '\n'.join(lines) + '\n'


def operator_status(height, block, app):
    return json.dumps({'jsonrpc': '2.0', 'id': -1, 'result': {'sync_info': {
        'latest_block_height': str(height), 'latest_block_hash': block, 'latest_app_hash': app}}})


def view(height, app_hash, s):
    """A status view as the node serializes it (consensus_settlement.rs
    query_validated; supply.rs SupplyResponse and DgtSupplyResponse):
    penalty_reserve is left out when "0" and governance_escrow when there is
    no governance accounting, and each response has fields the report does
    not read."""
    drt = {'accounting_version': 2, 'denomination': 'udrt', 'height': height,
           **{k: v for k, v in s['udrt'].items() if not k.startswith('pool_')},
           'pools': {k[5:]: v for k, v in s['udrt'].items() if k.startswith('pool_')},
           'issuance_timing': {'epoch': 1}, 'staking_reward_liabilities': {'total': '0'}}
    dgt = {'accounting_version': 1, 'denomination': 'udgt', 'height': height, 'cap': '1000000000000000',
           **{k: v for k, v in s['udgt'].items() if not (k in rc.DGT_OMITTED and v == '0')}}
    return {'version': 2, 'height': height, 'app_hash': app_hash, 'engine_hash': 'e' * 64,
            'issuance_timing': drt['issuance_timing'], 'supply': drt, 'dgt_supply': dgt,
            'last_commit_validators': [], 'effective_validators': [], 'next_validators': []}


def observations():
    """Two nodes that agree at heights 10 and 11: the header at 11 carries the
    application hash after 10, which the client's view at 10 reports."""
    s10, s11 = supply(), supply(emitted=55)
    hosts = {label: {'engine': {'10': {'block_hash': 'b10', 'header_app_hash': 'a9'},
                                '11': {'block_hash': 'b11', 'header_app_hash': 'a10'}},
                     'metrics': {'10': s10, '11': s11}} for label in ('validator-1', 'sentry-1')}
    return {'hosts': hosts, 'client': True,
            'views': {'10': {'app_hash': 'A10', 'supply': s10}, '11': {'app_hash': 'a11', 'supply': s11}}}


def status_of(obs):
    return rc.evaluate(obs, TOTALS)[1].status()


def rows(obs):
    return {r['check']: r for r in rc.evaluate(obs, TOTALS)[1].rows}


class ReadingTests(unittest.TestCase):
    def test_the_metrics_give_the_height_and_every_bucket(self):
        height, found = rc.metrics_supply('# TYPE x gauge\n' + prom(11, supply()))
        self.assertEqual((height, found), (11, supply()))

    def test_the_metrics_as_the_application_writes_them(self):
        # app_metrics.rs: TYPE lines, other metrics, every bucket always.
        text = ('# TYPE dytallix_app_height gauge\ndytallix_app_height 11\n'
                'dytallix_app_retained_from_height 1\n# TYPE dytallix_app_supply_udrt gauge\n'
                + prom(11, supply()).split('\n', 1)[1] + 'dytallix_app_gas_used_total 100\n')
        self.assertEqual(rc.metrics_supply(text), (11, supply()))
        self.assertIsNone(rc.metrics_supply('# before the first commit\ndytallix_app_retained_from_height 0\n'))

    def test_metrics_without_a_bucket_are_unreadable(self):
        s = supply()
        del s['udgt']['unbonding']
        with self.assertRaisesRegex(rc.Missing, 'udgt unbonding'):
            rc.metrics_supply(prom(11, s))
        with self.assertRaisesRegex(rc.Missing, 'dytallix_app_height'):
            rc.metrics_supply('dytallix_app_supply_udrt{bucket="total"} 1\n')

    def test_a_status_view_as_the_node_serializes_it(self):
        # The CI run that found this: dgt_supply without penalty_reserve.
        v = view(11, 'aa', supply())
        self.assertNotIn('penalty_reserve', v['dgt_supply'])
        self.assertNotIn('governance_escrow', v['dgt_supply'])
        self.assertEqual(rc.view_supply(v), supply())
        reserved = supply()
        reserved['udgt'].update(penalty_reserve='7', governance_escrow='3', liquid='890')
        self.assertEqual(rc.view_supply(view(11, 'aa', reserved)), reserved)
        escrow_none = view(11, 'aa', supply())
        escrow_none['dgt_supply']['governance_escrow'] = None
        self.assertEqual(rc.view_supply(escrow_none), supply())

    def test_a_status_view_without_a_required_field_is_unreadable(self):
        for part, name in (('dgt_supply', 'staked'), ('supply', 'burned'), ('supply', 'pools')):
            v = view(11, 'aa', supply())
            del v[part][name]
            with self.assertRaisesRegex(rc.Missing, name):
                rc.view_supply(v)
        v = view(11, 'aa', supply())
        del v['app_hash']
        with self.assertRaisesRegex(rc.Missing, 'app_hash'):
            rc.reading(v)

    def test_the_engine_status_is_lower_case(self):
        self.assertEqual(rc.engine_status(operator_status(7, 'AB', 'CD')), (7, 'ab', 'cd'))
        with self.assertRaises(rc.Missing):
            rc.engine_status(json.dumps({'result': {'node_info': {}}}))

    def test_a_balance_as_the_cli_prints_it(self):
        # balance.rs: the fields the report keeps, and the ones it drops.
        printed = {'address': 'a', 'height': 11, 'account_exists': True, 'udgt': '5', 'udrt': '6', 'dgt': '0.000005',
                   'drt': '0.000006', 'proof_verified': True, 'app_hash': 'ab', 'app_hash_source': 'next_header'}
        self.assertEqual(set(rc.balance_reading(printed)), set(rc.BALANCE))
        del printed['height']
        with self.assertRaisesRegex(rc.Missing, 'height'):
            rc.balance_reading(printed)

    def test_the_genesis_totals_are_the_account_sums(self):
        self.assertEqual(TOTALS, {'udrt': '1000', 'udgt': '1000'})


class RuleTests(unittest.TestCase):
    def test_a_sound_reading_breaks_nothing(self):
        self.assertEqual(rc.supply_rules(supply(), TOTALS), [])

    def broken(self, denomination, bucket, amount):
        s = copy.deepcopy(supply())
        s[denomination][bucket] = str(amount)
        return rc.supply_rules(s, TOTALS)

    def test_each_rule(self):
        self.assertIn('genesis + emitted - burned', self.broken('udrt', 'total', 1031)[0])
        self.assertIn('liquid + withheld + pools', self.broken('udrt', 'liquid', 1)[0])
        withheld = rc.supply_rules(dict(supply(), udrt=dict(supply()['udrt'], withheld_fees='5', liquid='995')), TOTALS)
        self.assertEqual(len(withheld), 1)
        self.assertIn('every fee is burned', withheld[0])
        self.assertTrue(any('native genesis allocation' in b for b in self.broken('udrt', 'genesis', 999)))
        self.assertIn('not the issued', self.broken('udgt', 'liquid', 899)[0])
        changed = rc.supply_rules(dict(supply(), udgt=dict(supply()['udgt'], issued='1001', liquid='901')), TOTALS)
        self.assertEqual(len(changed), 1)
        self.assertIn('changed from the genesis allocation', changed[0])


class EvaluateTests(unittest.TestCase):
    def test_agreeing_nodes_pass_at_their_newest_common_height(self):
        height, checks = rc.evaluate(observations(), TOTALS)
        self.assertEqual((height, checks.status()), (11, rc.PASS))
        self.assertEqual({r['status'] for r in checks.rows}, {rc.PASS})

    def test_no_common_height_is_inconclusive(self):
        obs = observations()
        del obs['hosts']['sentry-1']['metrics']['10'], obs['hosts']['sentry-1']['metrics']['11']
        self.assertEqual(rows(obs)['common_height']['status'], rc.INCONCLUSIVE)
        self.assertEqual(status_of(obs), rc.INCONCLUSIVE)

    def test_the_client_view_must_share_the_height(self):
        obs = observations()
        obs['views'] = {'9': obs['views']['10']}
        self.assertEqual(rc.common_height(obs), None)

    def test_two_blocks_at_one_height_fail(self):
        obs = observations()
        obs['hosts']['sentry-1']['engine']['10']['block_hash'] = 'other'
        self.assertEqual(rows(obs)['block_hashes']['status'], rc.FAIL)
        self.assertEqual(status_of(obs), rc.FAIL)

    def test_two_application_hashes_at_one_height_fail(self):
        obs = observations()
        obs['hosts']['sentry-1']['engine']['11']['header_app_hash'] = 'other'
        self.assertEqual(rows(obs)['app_hashes']['status'], rc.FAIL)
        obs = observations()
        obs['views']['10']['app_hash'] = 'other'
        self.assertIn('height 10', rows(obs)['app_hashes']['detail'])

    def test_supply_that_differs_between_nodes_fails(self):
        obs = observations()
        obs['hosts']['sentry-1']['metrics']['11'] = supply(emitted=56)
        self.assertEqual(rows(obs)['supply_equal']['status'], rc.FAIL)

    def test_a_broken_rule_on_any_reading_fails(self):
        obs = observations()
        bad = supply()
        bad['udrt']['total'] = '1031'
        obs['views']['10']['supply'] = bad
        self.assertEqual(rows(obs)['supply_rules']['status'], rc.FAIL)

    def test_emission_that_falls_fails(self):
        obs = observations()
        for host in obs['hosts'].values():
            host['metrics']['11'] = supply(emitted=40, burned=10)
        obs['views']['11']['supply'] = supply(emitted=40, burned=10)
        self.assertEqual(rows(obs)['monotonic']['status'], rc.FAIL)


class AccountTests(unittest.TestCase):
    def accounts(self, liquid_after=None, udrt=('500', '505'), heights=(11, 11, 12)):
        before, after = supply(emitted=55), supply(emitted=56)
        after['udrt']['liquid'] = before['udrt']['liquid'] if liquid_after is None else liquid_after
        after['udrt']['pool_staking'] = str(int(after['udrt']['total']) - int(after['udrt']['liquid']))
        obs = observations()
        obs['accounts'] = {
            'before': {'height': heights[0], 'app_hash': 'a11', 'supply': before},
            'after': {'height': heights[2], 'app_hash': 'a12', 'supply': after},
            'balances': [{'address': 'a', 'height': heights[1], 'account_exists': True, 'udrt': udrt[0],
                          'udgt': '500', 'app_hash': 'a11'},
                         {'address': 'b', 'height': heights[1], 'account_exists': True, 'udrt': udrt[1],
                          'udgt': '400', 'app_hash': 'a11'}]}
        return obs

    def test_the_tracked_accounts_hold_the_liquid_totals(self):
        self.assertEqual(rows(self.accounts())['accounts']['status'], rc.PASS)

    def test_a_missing_amount_fails(self):
        self.assertEqual(rows(self.accounts(udrt=('500', '504')))['accounts']['status'], rc.FAIL)

    def test_a_moving_total_is_inconclusive(self):
        self.assertEqual(rows(self.accounts(liquid_after='974'))['accounts']['status'], rc.INCONCLUSIVE)
        self.assertEqual(rows(self.accounts(heights=(11, 13, 12)))['accounts']['status'], rc.INCONCLUSIVE)

    def test_a_balance_proof_joins_the_application_hashes(self):
        obs = self.accounts()
        obs['accounts']['balances'][0]['app_hash'] = 'other'
        self.assertEqual(rows(obs)['app_hashes']['status'], rc.FAIL)


class FeeTests(unittest.TestCase):
    def with_ledger(self, charges=('3', '2'), created=1, from_height=8):
        obs = observations()
        obs['since'] = {'height': 8, 'supply': supply(burned=5)}
        obs['ledger'] = {'schema': rc.LEDGER_SCHEMA, 'from_height': from_height, 'account_creation_fee_udrt': '10',
                         'entries': [{'what': 'send', 'charge_udrt': charges[0], 'created_accounts': created},
                                     {'what': 'stake claim', 'charge_udrt': charges[1], 'created_accounts': 0}]}
        return obs

    def test_the_burns_are_the_fees_and_creation_fees(self):
        # 20 burned now, 5 at height 8: 3 + 2 charged and one account at 10.
        self.assertEqual(rows(self.with_ledger())['fees_and_burns']['status'], rc.PASS)

    def test_an_unexplained_burn_fails(self):
        self.assertEqual(rows(self.with_ledger(charges=('3', '1')))['fees_and_burns']['status'], rc.FAIL)
        self.assertEqual(rows(self.with_ledger(created=0))['fees_and_burns']['status'], rc.FAIL)

    def test_a_ledger_from_another_height_fails(self):
        self.assertEqual(rows(self.with_ledger(from_height=7))['fees_and_burns']['status'], rc.FAIL)

    def test_a_ledger_without_the_earlier_reading_is_inconclusive(self):
        obs = self.with_ledger()
        del obs['since']
        self.assertEqual(rows(obs)['fees_and_burns']['status'], rc.INCONCLUSIVE)

    def test_the_earlier_reading_comes_before(self):
        obs = self.with_ledger()
        obs['since']['height'] = 11
        obs['ledger']['from_height'] = 11
        self.assertEqual(rows(obs)['monotonic']['status'], rc.FAIL)

    def test_the_earlier_reading_from_a_view_or_evidence(self):
        self.assertEqual(rc.since_reading(view(8, 'aa', supply())), {'height': 8, 'supply': supply()})
        self.assertEqual(rc.since_reading({'schema': rc.SCHEMA, 'height': 9, 'supply': supply()}),
                         {'height': 9, 'supply': supply()})


class StateCheckTests(unittest.TestCase):
    def checked(self, result):
        obs = observations()
        obs['state_check'] = {'host': 'sentry-1', 'release': 'r', 'exit': 0, 'result': result}
        return rows(obs)['state_check']

    def test_a_pass_with_the_networks_hash_passes(self):
        self.assertEqual(self.checked({'result': 'pass', 'height': 10, 'app_hash': 'A10'})['status'], rc.PASS)

    def test_a_failed_check_fails(self):
        row = self.checked({'result': 'fail', 'class': 'supply', 'error': 'rule'})
        self.assertEqual((row['status'], 'supply' in row['detail']), (rc.FAIL, True))
        self.assertEqual(self.checked({'result': 'empty'})['status'], rc.FAIL)

    def test_another_hash_fails_and_an_unread_height_is_inconclusive(self):
        self.assertEqual(self.checked({'result': 'pass', 'height': 10, 'app_hash': 'ff'})['status'], rc.FAIL)
        self.assertEqual(self.checked({'result': 'pass', 'height': 4, 'app_hash': 'ff'})['status'], rc.INCONCLUSIVE)


class EvidenceTests(unittest.TestCase):
    def test_the_evidence_verifies_and_shows_any_edit(self):
        record = rc.evidence('dytallix-staging-1', observations(), GENESIS_BYTES, 's', 'f')
        self.assertEqual((record['status'], record['height'], record['block_hash']), (rc.PASS, 11, 'b11'))
        self.assertEqual(record['genesis']['udrt'], '1000')
        self.assertEqual(rc.verify(json.loads(json.dumps(record)), GENESIS_BYTES), rc.PASS)
        edited = json.loads(json.dumps(record))
        edited['observations']['hosts']['sentry-1']['engine']['11']['block_hash'] = 'x'
        with self.assertRaises(rc.Failed):
            rc.verify(edited, GENESIS_BYTES)
        with self.assertRaises(rc.Failed):
            rc.verify(record, GENESIS_BYTES + b' ')

    def test_checks_that_do_not_follow_from_the_readings_are_refused(self):
        record = rc.evidence('c', observations(), GENESIS_BYTES, 's', 'f')
        body = {k: v for k, v in record.items() if k != 'sha256'}
        body['status'] = rc.FAIL
        forged = dict(body, sha256=rc.digest(rc.canonical(body)))
        with self.assertRaises(rc.Failed):
            rc.verify(forged, GENESIS_BYTES)

    def test_a_failing_run_still_records_its_evidence(self):
        obs = observations()
        obs['hosts']['sentry-1']['engine']['10']['block_hash'] = 'other'
        record = rc.evidence('c', obs, GENESIS_BYTES, 's', 'f')
        self.assertEqual(rc.verify(record, GENESIS_BYTES), rc.FAIL)


class FakeHost:
    """A node whose chain advances one block per reading."""

    def __init__(self, height=10, block='b', app='a', stopped=False):
        self.height, self.block, self.app, self.stopped, self.commands = height, block, app, stopped, []

    def run(self, args, stdin=None, timeout=600, check=True):
        if tuple(args) == rc.OPERATOR_STATUS:
            if self.stopped:
                return 1, '', 'stopped'
            self.height += 1
            return 0, operator_status(self.height, f'{self.block}{self.height}', f'{self.app}{self.height - 1}'), ''
        self.commands.append(list(args))
        if args[:2] == ['systemctl', 'show']:
            return 0, '{ path=/opt/dytallix/r1/bin/dytallix-native-supervisor ; argv[]=... }\n', ''
        if args[:2] == ['systemctl', 'stop']:
            self.stopped = True
        if args[:2] == ['systemctl', 'start']:
            self.stopped = False
        if args[0].endswith('dytallix-state-check'):
            return 0, json.dumps({'schema': 1, 'result': 'pass', 'height': 12, 'app_hash': 'a12'}), ''
        return 0, '', ''

    def read(self, path):
        if self.stopped:
            raise OSError('stopped')
        return prom(self.height, supply()).encode()


class FakeClient:
    def __init__(self, host):
        self.host = host

    def status(self):
        if self.host.stopped:
            raise rc.Failed('the channel is closed')
        return view(self.host.height, f'a{self.host.height}', supply())

    def balance(self, address):
        return {'address': address, 'height': self.host.height, 'account_exists': True, 'udrt': '0', 'udgt': '0',
                'app_hash': f'a{self.host.height}', 'extra': 'dropped'}


class CollectorTests(unittest.TestCase):
    def collector(self, client=False):
        hosts = {'validator-1': FakeHost(), 'sentry-1': FakeHost()}
        quiet = []
        return rc.Collector(hosts, FakeClient(hosts['sentry-1']) if client else None, clock=iter(range(10**6)).__next__,
                            sleep=lambda _: None, say=quiet.append), hosts

    def test_it_reads_until_every_node_shares_a_height(self):
        collector, _ = self.collector()
        self.assertEqual(collector.until_common(30), 11)
        self.assertEqual(set(collector.observations['hosts']['sentry-1']['engine']), {'11'})

    def test_a_node_that_never_answers_gives_no_height(self):
        collector, hosts = self.collector()
        hosts['sentry-1'].stopped = True
        self.assertIsNone(collector.until_common(5))
        self.assertEqual(collector.observations['hosts']['sentry-1'], {'engine': {}, 'metrics': {}})

    def test_the_accounts_are_read_inside_a_window(self):
        collector, _ = self.collector(client=True)
        self.assertTrue(collector.accounts(['a', 'b']))
        recorded = collector.observations['accounts']
        self.assertEqual([b['address'] for b in recorded['balances']], ['a', 'b'])
        self.assertNotIn('extra', recorded['balances'][0])

    def test_the_state_check_stops_copies_starts_and_checks(self):
        collector, hosts = self.collector(client=True)
        result = collector.state_check('sentry-1', '/var/tmp/copy', wait=2)
        self.assertEqual(result['result'], 'pass')
        commands = [c[:2] for c in hosts['sentry-1'].commands]
        self.assertEqual(commands, [['systemctl', 'show'], ['systemctl', 'stop'], ['rm', '-rf'],
                                    ['cp', '-a'], ['systemctl', 'start'],
                                    ['/opt/dytallix/r1/bin/dytallix-state-check', '--config'], ['rm', '-rf']])
        check = hosts['sentry-1'].commands[5]
        self.assertEqual(check[1:], ['--config', '/etc/dytallix/r1/application-config.json', '--genesis',
                                     '/etc/dytallix/r1/native-genesis.json', '--db', '/var/tmp/copy'])
        self.assertEqual(hosts['sentry-1'].commands[3], ['cp', '-a', rc.APPDB, '/var/tmp/copy'])
        self.assertEqual(collector.observations['state_check']['release'], 'r1')
        # The validator's headers, read while the sentry was stopped, carry the
        # hash the copy reports.
        collector.until_common(30)
        found = {r['check']: r['status'] for r in rc.evaluate(collector.observations, TOTALS)[1].rows}
        self.assertEqual(found['state_check'], rc.PASS)

    def test_the_node_starts_again_when_the_copy_fails(self):
        collector, hosts = self.collector()

        def failing(args, stdin=None, timeout=600, check=True, run=hosts['sentry-1'].run):
            if args[0] == 'cp':
                raise rc.Failed('no space')
            return run(args, stdin, timeout, check)
        hosts['sentry-1'].run = failing
        with self.assertRaises(rc.Failed):
            collector.state_check('sentry-1', '/var/tmp/copy', wait=1)
        self.assertFalse(hosts['sentry-1'].stopped)

    def test_collect_with_the_client_and_its_real_views_passes(self):
        hosts = {'validator-1': FakeHost(), 'sentry-1': FakeHost()}
        genesis = json.dumps({'accounts': [{'address': 'a', 'balances': {'udrt': '1000', 'udgt': '1000'}}]}).encode()
        record = rc.collect('c', hosts, genesis, client=FakeClient(hosts['sentry-1']),
                            since=view(5, 'aa', supply()), timeout=30, say=lambda _: None)
        self.assertEqual(record['status'], rc.PASS, record['checks'])
        self.assertEqual(record['observations']['since']['height'], 5)

    def test_a_view_missing_a_required_field_is_inconclusive_not_a_traceback(self):
        hosts = {'validator-1': FakeHost(), 'sentry-1': FakeHost()}
        client = FakeClient(hosts['sentry-1'])
        real = client.status

        def without_staked():
            v = real()
            del v['dgt_supply']['staked']
            return v
        client.status = without_staked
        genesis = json.dumps({'accounts': [{'address': 'a', 'balances': {'udrt': '1000', 'udgt': '1000'}}]}).encode()
        record = rc.collect('c', hosts, genesis, client=client, addresses=['a'], since={'height': 5},
                            timeout=5, say=lambda _: None)
        self.assertEqual(record['status'], rc.INCONCLUSIVE)
        readings = next(r for r in record['checks'] if r['check'] == 'readings')
        self.assertEqual(readings['status'], rc.INCONCLUSIVE)
        self.assertIn('client: the status view\'s dgt_supply has no staked', readings['detail'])
        self.assertIn('the earlier reading: the status view has no app_hash', readings['detail'])
        self.assertEqual(rc.verify(json.loads(json.dumps(record)), genesis), rc.INCONCLUSIVE)

    def test_collect_records_a_pass(self):
        hosts = {'validator-1': FakeHost(), 'sentry-1': FakeHost()}
        record = rc.collect('c', hosts, json.dumps({'accounts': [
            {'address': 'a', 'balances': {'udrt': '1000', 'udgt': '1000'}}]}).encode(), timeout=5, say=lambda _: None)
        self.assertEqual(record['status'], rc.PASS, record['checks'])


class StagingTests(unittest.TestCase):
    def test_the_staging_network_reads_the_same_operator_status(self):
        import staging_network
        self.assertIs(staging_network.OPERATOR_STATUS, rc.OPERATOR_STATUS)


if __name__ == '__main__':
    unittest.main()
