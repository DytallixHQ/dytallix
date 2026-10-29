"""The sentry's backup job (disaster recovery v1): which snapshot it copies,
how the upload key reaches curl, the copy's parts and manifest, what it
records, and the restore's join of the parts. The signer and curl are
stand-ins; the encryption is `dytallix-root-sign`'s own tests."""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'host'))
import backup  # noqa: E402
import host_install  # noqa: E402

SIGNER = r'''#!PYTHON
import sys
args = sys.argv[1:]
flags = dict(zip(args[1::2], args[2::2]))
assert args[0] == 'backup-seal'
open(flags['-out'], 'xb').write(b'encrypted copy of ' + flags['-in'].encode() + b' with ' + flags['-light-blocks'].encode())
print('wrote', flags['-out'])
'''
CURL = r'''#!PYTHON
import json, os, sys
log = os.environ['CURL_LOG']
config = sys.stdin.read()
upload = next(l.split('"')[1] for l in config.splitlines() if l.startswith('upload-file'))
url = next(l.split('"')[1] for l in config.splitlines() if l.startswith('url'))
if os.environ.get('CURL_FAIL', '\0') in url:
    sys.exit(22)
entry = {'argv': sys.argv[1:], 'config': config, 'url': url, 'copy': open(upload, 'rb').read().decode()}
with open(log, 'a') as file:
    file.write(json.dumps(entry) + '\n')
'''
UPLOAD = {'schema': 'dytallix.backup-upload.v1', 'endpoint': 'https://storage.example', 'bucket': 'copies',
          'region': 'auto', 'prefix': 'staging/', 'access_key_id': 'AKIDEXAMPLE', 'secret_access_key': 'c2VjcmV0'}


class BackupJobTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.snapshots = root / 'snapshots'
        self.light_blocks = root / 'light-blocks'
        self.light_blocks.mkdir()
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
            'light_blocks': str(self.light_blocks),
            'signer': str(tools / 'signer'), 'code_file': str(root / 'code'), 'upload_file': str(self.upload_path),
            'state': str(self.state), 'curl': str(tools / 'curl')}))

    def snapshot(self, height, complete=True, light_blocks=True):
        directory = self.snapshots / f'{height:020}'
        directory.mkdir()
        if complete:
            (directory / 'metadata.json').write_text('{}')
        if light_blocks:
            (self.light_blocks / f'{height:020}').mkdir()

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def run_job(self, **options):
        lines = []
        return backup.run(self.config, out=lines.append, **options), lines

    def sealed(self, height):
        """The stand-in signer's copy of that height."""
        return (f'encrypted copy of {self.snapshots / f"{height:020}"} with '
                f'{self.light_blocks / f"{height:020}"}').encode()

    def fetch(self, result, folder):
        """The uploaded objects of one copy, as the founder fetches them."""
        folder.mkdir()
        base = f'https://storage.example/copies/{result["copy"]}'
        for call in self.calls():
            if call['url'].startswith(base):
                (folder / call['url'][len(base):]).write_bytes(call['copy'].encode())
        return folder

    def test_uploads_the_newest_complete_snapshot_once(self):
        result, _ = self.run_job()
        self.assertIsNone(result)  # no snapshot yet
        self.snapshot(17280)
        self.snapshot(34560)
        self.snapshot(51840, complete=False)  # still being written
        self.snapshot(69120, light_blocks=False)  # its light blocks not yet written
        result, _ = self.run_job()
        self.assertEqual(result['height'], 34560)
        copy = self.sealed(34560)
        self.assertEqual(result['sha256'], hashlib.sha256(copy).hexdigest())
        self.assertEqual(result['copy'], f'staging/dytallix-staging-1/00000000000000034560-{result["sha256"]}/')
        self.assertEqual((result['bytes'], result['parts']), (len(copy), 1))
        part, manifest = self.calls()
        # The upload key reaches curl on its standard input only.
        for call in (part, manifest):
            self.assertEqual(call['argv'], ['--config', '-'])
            self.assertNotIn('c2VjcmV0', ' '.join(call['argv']))
            self.assertIn('user = "AKIDEXAMPLE:c2VjcmV0"', call['config'])
            self.assertIn('aws-sigv4 = "aws:amz:auto:s3"', call['config'])
        # One part, then the manifest, last.
        base = f'https://storage.example/copies/{result["copy"]}'
        self.assertEqual([part['url'], manifest['url']], [base + 'part-0000.bin', base + 'manifest.json'])
        self.assertEqual(part['copy'].encode(), copy)
        self.assertEqual(json.loads(manifest['copy']), {
            'schema': 'dytallix.backup-parts.v1', 'chain_id': 'dytallix-staging-1', 'height': 34560,
            'copy_bytes': len(copy), 'copy_sha256': result['sha256'], 'part_bytes': 1 << 30,
            'parts': [{'name': 'part-0000.bin', 'bytes': len(copy), 'sha256': result['sha256']}]})
        self.assertEqual((self.state / 'last-uploaded').read_text(), '34560\n')
        self.assertEqual(list((self.state / 'scratch').iterdir()), [], 'the scratch copy was left behind')
        # Nothing new: no second upload.
        result, lines = self.run_job()
        self.assertIsNone(result)
        self.assertIn('NOTHING_NEW', lines[0])
        self.assertEqual(len(self.calls()), 2)
        # The next snapshot goes up, then the one whose light blocks arrive.
        (self.snapshots / f'{51840:020}' / 'metadata.json').write_text('{}')
        result, _ = self.run_job()
        self.assertEqual(result['height'], 51840)
        (self.light_blocks / f'{69120:020}').mkdir()
        result, _ = self.run_job()
        self.assertEqual(result['height'], 69120)

    def test_a_large_copy_goes_up_in_parts_and_joins_back(self):
        self.snapshot(17280)
        copy = self.sealed(17280)
        result, _ = self.run_job(part_bytes=16)
        count = -(-len(copy) // 16)
        self.assertGreater(count, 3)
        self.assertEqual(result['parts'], count)
        calls = self.calls()
        base = f'https://storage.example/copies/{result["copy"]}'
        self.assertEqual([c['url'] for c in calls],
                         [f'{base}part-{i:04}.bin' for i in range(count)] + [base + 'manifest.json'])
        self.assertEqual([c['copy'].encode() for c in calls[:-1]], [copy[i:i + 16] for i in range(0, len(copy), 16)])
        manifest = json.loads(calls[-1]['copy'])
        self.assertEqual(manifest['part_bytes'], 16)
        self.assertEqual([p['sha256'] for p in manifest['parts']],
                         [hashlib.sha256(copy[i:i + 16]).hexdigest() for i in range(0, len(copy), 16)])
        # The restore checks every part, joins them in order, removing each
        # once appended, and checks the joined copy.
        folder = self.fetch(result, Path(self.tmp.name) / 'fetched')
        printed = []
        joined = host_install.join_parts(folder, 'dytallix-staging-1', out=printed.append)
        self.assertEqual(joined, folder / 'copy.bin')
        self.assertEqual(joined.read_bytes(), copy)
        self.assertEqual(sorted(p.name for p in folder.iterdir()), ['copy.bin', 'manifest.json'])
        self.assertIn(f'{count} parts', printed[0])
        # A second run (a retried restore) reuses the joined copy.
        self.assertEqual(host_install.join_parts(folder, 'dytallix-staging-1', out=printed.append), joined)

    def test_the_join_refuses_a_copy_that_does_not_match_its_manifest(self):
        self.snapshot(17280)
        result, _ = self.run_job(part_bytes=16)
        cases = {
            'missing': (lambda f, m: (f / 'part-0002.bin').unlink(), 'part-0002.bin is missing'),
            'altered': (lambda f, m: (f / 'part-0001.bin').write_bytes(b'x' * 16), 'part-0001.bin does not match'),
            'truncated': (lambda f, m: (f / 'part-0000.bin').write_bytes(b'short'), 'part-0000.bin does not match'),
            'reordered': (lambda f, m: m['parts'].reverse(), 'in order'),
            'escaping': (lambda f, m: m['parts'][0].update(name='../part-0000.bin'), 'in order'),
            'short': (lambda f, m: m['parts'].pop(), 'add up'),
            'other chain': (lambda f, m: m.update(chain_id='dytallix-mainnet-1'), 'not of dytallix-staging-1'),
            'other schema': (lambda f, m: m.update(schema='dytallix.backup.v1'), 'not a copy'),
            'joined copy': (lambda f, m: (f / 'copy.bin').write_bytes(b'stale'), 'not the copy the manifest names'),
        }
        for name, (change, message) in cases.items():
            with self.subTest(case=name):
                folder = self.fetch(result, Path(self.tmp.name) / name.replace(' ', '-'))
                manifest = json.loads((folder / 'manifest.json').read_text())
                change(folder, manifest)
                (folder / 'manifest.json').write_text(json.dumps(manifest))
                before = sorted(p.name for p in folder.iterdir())
                with self.assertRaisesRegex(host_install.Refused, message):
                    host_install.join_parts(folder, 'dytallix-staging-1', out=lambda _: None)
                # Nothing is joined or removed before every part checks.
                self.assertEqual(sorted(p.name for p in folder.iterdir()), before)

    def test_a_failed_upload_records_nothing_and_leaves_no_copy(self):
        self.snapshot(17280)
        # The first part fails, then (another run) only the manifest.
        for failing, uploaded in (('part-0000', 0), ('manifest.json', 1)):
            with self.subTest(failing=failing):
                os.environ['CURL_FAIL'] = failing
                self.addCleanup(os.environ.pop, 'CURL_FAIL', None)
                with self.assertRaisesRegex(backup.Failed, f'upload failed .*{failing}'):
                    self.run_job()
                self.assertFalse((self.state / 'last-uploaded').exists())
                self.assertEqual(list((self.state / 'scratch').iterdir()), [])
                self.assertFalse(any(c['url'].endswith('manifest.json') for c in self.calls()))
                self.assertEqual(len(self.calls()), uploaded)

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
