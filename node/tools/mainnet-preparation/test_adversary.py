"""The adversarial generator (simulation plan, T4): its byte-level mutations
change only the field under test, and a case's status follows from the node's
admission outcome. The live submission through the client channel is exercised
by the Host install workflow's adversary step; here the outcomes are synthetic
so the tests cover T4's logic alone."""
import base64
import json
import os
import unittest

import adversary

ENGINE = json.dumps({'genesis_time': '2026-10-12T14:00:00Z', 'chain_id': 'dytallix-staging-1'}).encode()
NATIVE = json.dumps({'accounts': [{'address': 'a'}]}).encode()


def transport(envelope):
    return json.dumps({'type': 'ordinary_v2', 'envelope_base64': base64.b64encode(envelope).decode()}).encode()


class Mutations(unittest.TestCase):
    def setUp(self):
        self.envelope = os.urandom(4000)  # larger than one 3309-byte signature
        self.tx = transport(self.envelope)

    def test_corrupt_signature_changes_one_trailing_byte_only(self):
        out = adversary.corrupt_signature(self.tx)
        after = base64.b64decode(json.loads(out)['envelope_base64'])
        self.assertEqual(len(after), len(self.envelope))  # length preserved
        differ = [i for i in range(len(after)) if after[i] != self.envelope[i]]
        self.assertEqual(len(differ), 1)  # exactly one byte
        self.assertGreaterEqual(differ[0], len(self.envelope) - adversary.MLDSA65_SIGNATURE_BYTES)  # inside the signature

    def test_corrupt_signature_needs_a_full_envelope(self):
        with self.assertRaises(adversary.Failed):
            adversary.corrupt_signature(transport(os.urandom(100)))

    def test_corrupt_encoding_breaks_the_base64(self):
        out = json.loads(adversary.corrupt_encoding(self.tx))
        with self.assertRaises(Exception):
            base64.b64decode(out['envelope_base64'], validate=True)

    def test_oversized_envelope_decodes_to_the_asked_size(self):
        out = json.loads(adversary.oversized_envelope(300000))
        self.assertEqual(len(base64.b64decode(out['envelope_base64'])), 300000)


def case(name, code=1, committed=False, log='', expected_reason=None):
    row = {'name': name, 'code': code, 'committed': committed, 'log': log}
    if expected_reason is not None:
        row['expected_reason'] = expected_reason
    return row


class Evaluate(unittest.TestCase):
    def status(self, c):
        return adversary.evaluate(c)[0]

    def test_control_passes_only_when_admitted_and_committed(self):
        self.assertEqual(self.status(case('positive_control', code=0, committed=True)), adversary.PASS)
        self.assertEqual(self.status(case('positive_control', code=0, committed=False)), adversary.FAIL)
        self.assertEqual(self.status(case('positive_control', code=1, committed=False)), adversary.FAIL)

    def test_a_refuse_case_passes_on_a_node_refusal(self):
        self.assertEqual(self.status(case('invalid_signature', code=1)), adversary.PASS)

    def test_a_refuse_case_fails_if_admitted_or_committed(self):
        self.assertEqual(self.status(case('stale_nonce', code=0)), adversary.FAIL)
        self.assertEqual(self.status(case('stale_nonce', code=1, committed=True)), adversary.FAIL)

    def test_a_transport_or_uncertain_outcome_never_passes(self):
        self.assertEqual(self.status(case('decode_failure', code=-1)), adversary.FAIL)
        self.assertEqual(self.status(case('decode_failure', code=None)), adversary.FAIL)

    def test_the_wrong_reason_fails(self):
        self.assertEqual(self.status(case('oversized', code=1, log='something else',
                                          expected_reason='exceeds profile')), adversary.FAIL)
        self.assertEqual(self.status(case('oversized', code=1, log='ordinary envelope exceeds profile',
                                          expected_reason='exceeds profile')), adversary.PASS)

    def test_oversized_may_be_refused_before_checktx(self):
        # The mempool/RPC size limit returns a JSON-RPC error (code -1), not CheckTx.
        self.assertEqual(self.status(case('oversized', code=-1, log='Tx too large. Max size is 262144',
                                          expected_reason='too large')), adversary.PASS)
        # A bare transport error (no size reason) does not pass.
        self.assertEqual(self.status(case('oversized', code=-1, log='connection reset',
                                          expected_reason='too large')), adversary.FAIL)
        # Only oversized gets the before-CheckTx allowance.
        self.assertEqual(self.status(case('invalid_signature', code=-1, log='Tx too large',
                                          expected_reason='too large')), adversary.FAIL)

    def test_an_unregistered_case_is_refused(self):
        with self.assertRaises(adversary.Failed):
            adversary.evaluate(case('mystery'))


class Evidence(unittest.TestCase):
    def build(self, cases):
        return adversary.build_run('dytallix-staging-1', NATIVE, ENGINE, cases, 'sentry-1')

    def good_run(self):
        return [case('invalid_signature'), case('decode_failure'), case('stale_nonce'),
                case('duplicate'), case('resigned_copy'), case('oversized'),
                case('positive_control', code=0, committed=True)]

    def test_build_then_verify_roundtrips_pass(self):
        record = self.build(self.good_run())
        self.assertEqual(record['status'], adversary.PASS)
        self.assertEqual(adversary.verify(record, NATIVE, ENGINE), adversary.PASS)

    def test_run_fails_if_any_case_fails(self):
        cases = self.good_run()
        cases[0] = case('invalid_signature', code=0)  # admitted: a real miss
        record = self.build(cases)
        self.assertEqual(record['status'], adversary.FAIL)
        self.assertEqual(adversary.verify(record, NATIVE, ENGINE), adversary.FAIL)

    def test_a_run_without_a_control_is_refused(self):
        with self.assertRaises(adversary.Failed):
            adversary.verify(self.build([case('invalid_signature')]), NATIVE, ENGINE)

    def test_a_tampered_outcome_is_caught(self):
        record = self.build(self.good_run())
        record['cases'][0]['code'] = 0
        with self.assertRaises(adversary.Failed):
            adversary.verify(record, NATIVE, ENGINE)

    def test_a_relabelled_case_is_caught(self):
        record = self.build(self.good_run())
        record['cases'][0]['expectation'] = 'admit'
        record['sha256'] = adversary.digest(adversary.canonical({k: v for k, v in record.items() if k != 'sha256'}))
        with self.assertRaises(adversary.Failed):
            adversary.verify(record, NATIVE, ENGINE)

    def test_a_swapped_genesis_is_caught(self):
        record = self.build(self.good_run())
        with self.assertRaises(adversary.Failed):
            adversary.verify(record, NATIVE, json.dumps({'genesis_time': 'x'}).encode())


if __name__ == '__main__':
    unittest.main()
