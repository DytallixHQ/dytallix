"""The sentry's backup job (disaster recovery v1): which snapshot it copies,
how the upload key reaches curl, and what it records. The signer and curl
are stand-ins; the encryption is `dytallix-root-sign`'s own tests."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'host'))
import backup  # noqa: E402

SIGNER = r'''#!PYTHON
import sys
args = sys.argv[1:]
flags = dict(zip(args[1::2], args[2::2]))
assert args[0] == 'backup-seal'
open(flags['-out'], 'xb').write(b'encrypted copy of ' + flags['-in'].encode())
print('wrote', flags['-out'])
'''
CURL = r'''#!PYTHON
import json, os, sys
log = os.environ['CURL_LOG']
config = sys.stdin.read()
upload = next(l.split('"')[1] for l in config.splitlines() if l.startswith('upload-file'))
entry = {'argv': sys.argv[1:], 'config': config, 'copy': open(upload, 'rb').read().decode()}
with open(log, 'a') as file:
    file.write(json.dumps(entry) + '\n')
sys.exit(int(os.environ.get('CURL_EXIT', '0')))
'''
UPLOAD = {'schema': 'dytallix.backup-upload.v1', 'endpoint': 'https://storage.example', 'bucket': 'copies',
          'region': 'auto', 'prefix': 'staging/', 'access_key_id': 'AKIDEXAMPLE', 'secret_access_key': 'c2VjcmV0'}


class BackupJobTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.snapshots = root / 'snapshots'
        self.state = root / 'state'
        (self.state / 'scratch').mkdir(parents=True)
        self.snapshots.mkdir()
        tools = root / 'tools'
        tools.mkdir()
        for name, text in (('signer', SIGNER), ('curl', CURL)):
            (tools / name).write_text(text.replace('PYTHON', sys.executable))
            (tools / name).chmod(0o755)
        self.log = root / 'curl.log'
        os.environ['CURL_LOG'] = str(self.log)
        self.addCleanup(os.environ.pop, 'CURL_LOG')
        self.upload_path = root / 'upload.json'
        self.upload_path.write_text(json.dumps(UPLOAD))
        self.config = root / 'backup.json'
        self.config.write_text(json.dumps({
            'schema': 'dytallix.host-backup.v1', 'chain_id': 'dytallix-staging-1', 'snapshots': str(self.snapshots),
            'signer': str(tools / 'signer'), 'code_file': str(root / 'code'), 'upload_file': str(self.upload_path),
            'state': str(self.state), 'curl': str(tools / 'curl')}))

    def snapshot(self, height, complete=True):
        directory = self.snapshots / f'{height:020}'
        directory.mkdir()
        if complete:
            (directory / 'metadata.json').write_text('{}')

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def run_job(self):
        lines = []
        return backup.run(self.config, out=lines.append), lines

    def test_uploads_the_newest_complete_snapshot_once(self):
        result, _ = self.run_job()
        self.assertIsNone(result)  # no snapshot yet
        self.snapshot(17280)
        self.snapshot(34560)
        self.snapshot(51840, complete=False)  # still being written
        result, _ = self.run_job()
        self.assertEqual(result['height'], 34560)
        self.assertRegex(result['object'], r'^staging/dytallix-staging-1/00000000000000034560-[0-9a-f]{64}\.bin$')
        (call,) = self.calls()
        # The upload key reaches curl on its standard input only.
        self.assertEqual(call['argv'], ['--config', '-'])
        self.assertNotIn('c2VjcmV0', ' '.join(call['argv']))
        self.assertIn('user = "AKIDEXAMPLE:c2VjcmV0"', call['config'])
        self.assertIn('aws-sigv4 = "aws:amz:auto:s3"', call['config'])
        self.assertIn(f'url = "https://storage.example/copies/{result["object"]}"', call['config'])
        self.assertTrue(call['copy'].endswith('00000000000000034560'))
        self.assertEqual((self.state / 'last-uploaded').read_text(), '34560\n')
        self.assertEqual(list((self.state / 'scratch').iterdir()), [], 'the scratch copy was left behind')
        # Nothing new: no second upload.
        result, lines = self.run_job()
        self.assertIsNone(result)
        self.assertIn('NOTHING_NEW', lines[0])
        self.assertEqual(len(self.calls()), 1)
        # The next snapshot goes up.
        (self.snapshots / f'{51840:020}' / 'metadata.json').write_text('{}')
        result, _ = self.run_job()
        self.assertEqual(result['height'], 51840)

    def test_a_failed_upload_records_nothing_and_leaves_no_copy(self):
        self.snapshot(17280)
        os.environ['CURL_EXIT'] = '22'
        self.addCleanup(os.environ.pop, 'CURL_EXIT')
        with self.assertRaisesRegex(backup.Failed, 'upload failed'):
            self.run_job()
        self.assertFalse((self.state / 'last-uploaded').exists())
        self.assertEqual(list((self.state / 'scratch').iterdir()), [])

    def test_refuses_an_unsafe_upload_key(self):
        cases = {'endpoint': 'http://storage.example', 'bucket': 'a"b', 'secret_access_key': 'x\nurl = "evil"',
                 'prefix': '../up'}
        for field, value in cases.items():
            with self.subTest(field=field):
                self.upload_path.write_text(json.dumps(dict(UPLOAD, **{field: value})))
                with self.assertRaises(backup.Failed):
                    backup.load(self.config)
        # A stand-in store on this host may use plain HTTP.
        self.upload_path.write_text(json.dumps(dict(UPLOAD, endpoint='http://127.0.0.1:9000')))
        backup.load(self.config)


if __name__ == '__main__':
    unittest.main()
