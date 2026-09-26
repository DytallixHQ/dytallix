"""Failure-path and classifier tests. No network or production process is used."""
import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import check_boundary as boundary


class BoundaryTests(unittest.TestCase):
 def args(self, binary, **extra):
  values=dict(profile=boundary.PROFILE,binary=str(binary),module_dir=str(binary.parent),tags='',expected_sha256=None,rebuild=False)
  values.update(extra)
  return argparse.Namespace(**values)

 def test_missing_binary_fails(self):
  with tempfile.TemporaryDirectory() as temp:
   report=boundary.inspect(self.args(Path(temp)/'absent'))
   self.assertEqual(report['boundary_status'],'FAIL')
   self.assertIn('binary is missing',report['errors'][0])
   self.assertEqual(report['launch_status'],'NO_GO')

 def test_tampered_binary_fails_before_tool_invocation(self):
  with tempfile.TemporaryDirectory() as temp:
   binary=Path(temp)/'engine';binary.write_bytes(b'original')
   approved=boundary.digest(binary);binary.write_bytes(b'tampered')
   report=boundary.inspect(self.args(binary,expected_sha256=approved))
   self.assertEqual(report['errors'],['binary SHA256 mismatch'])

 def test_unknown_profile_fails(self):
  report=boundary.inspect(self.args(Path('/absent'),profile='allow-everything'))
  self.assertEqual(report['errors'],['unknown profile'])

 def test_unknown_tags_fail(self):
  report=boundary.inspect(self.args(Path('/absent'),tags='dytallix_pqc_only,legacy'))
  self.assertEqual(report['errors'],['unknown build tag selection'])

 def test_ipc_tags_do_not_skip_artifact_checks(self):
  report=boundary.inspect(self.args(Path('/absent'),tags=boundary.IPC_TAG))
  self.assertEqual(report['errors'],['binary is missing or not a regular file'])
  report=boundary.inspect(self.args(Path('/absent'),tags='dytallix_pqc_ipc'))
  self.assertEqual(report['errors'],['unknown build tag selection'])

 def test_empty_or_stripped_symbols_fail(self):
  for symbols in ('', 'reading engine: no symbol section', 'abc T main.main\n'):
   with self.assertRaises(ValueError):boundary.parse_symbols(symbols)

 def test_malformed_symbols_fail(self):
  with self.assertRaises(ValueError):boundary.parse_symbols('123 T runtime.main\nnot a symbol line\n')

 def test_classical_packages_fail(self):
  for package in ('crypto/tls','crypto/x509/pkix','crypto/rsa','crypto/internal/fips140/ed25519',
                  'github.com/cometbft/cometbft/crypto/ed25519',
                  'github.com/libp2p/go-libp2p/p2p/security/noise',
                  'github.com/pion/dtls/v3', 'golang.org/x/crypto/curve25519'):
   self.assertTrue(boundary.classify_package(package),package)

 def test_boring_provider_is_review_item_not_algorithm_implementation(self):
  for package in ('crypto/internal/boring','crypto/internal/boring/bbig','crypto/internal/boring/sig'):
   self.assertEqual(boundary.classify_package(package),[])
   self.assertEqual(boundary.classify_provider_package(package),['provider_container_requires_review'])
  for symbol in ('crypto/internal/boring/sig.StandardCrypto.abi0','crypto/internal/boring.(*PrivateKeyECDH).PublicKey'):
   self.assertEqual(boundary.classify_symbol(symbol),[])
   self.assertTrue(boundary.classify_provider_symbol(symbol))

 def test_fips140_implementations_remain_prohibited(self):
  for suffix in ('ed25519','edwards25519','rsa','ecdsa','ecdh'):
   package='crypto/internal/fips140/'+suffix
   self.assertTrue(boundary.classify_package(package))
   self.assertTrue(boundary.classify_symbol(package+'.Verify'))

 def test_pqc_hash_and_symmetric_packages_not_classical(self):
  for package in ('crypto/sha256','crypto/aes','crypto/internal/fips140/mlkem',
                  'github.com/cloudflare/circl/sign/mldsa/mldsa65',
                  'github.com/cloudflare/circl/kem/mlkem/mlkem768',
                  'golang.org/x/crypto/chacha20poly1305'):
   self.assertEqual(boundary.classify_package(package),[],package)

 def test_methods_detected_but_protobuf_enum_not_counted(self):
  for symbol in ('crypto/rsa.(*PrivateKey).Sign','crypto/internal/fips140/ed25519.Verify',
                 'github.com/cometbft/cometbft/p2p/conn.MakeSecretConnection'):
   self.assertTrue(boundary.classify_symbol(symbol),symbol)
  self.assertEqual(boundary.classify_symbol('github.com/cometbft/cometbft/api/crypto.PublicKey_Ed25519'),[])

 def test_source_and_lock_tamper_change_manifest(self):
  with tempfile.TemporaryDirectory() as temp:
   directory=Path(temp);(directory/'main.go').write_text('package main')
   (directory/'go.mod').write_text('module example');(directory/'go.sum').write_text('lock')
   pkg={'Dir':temp,'GoFiles':['main.go'],'Module':{'Dir':temp}}
   before=boundary.canonical_hash(boundary.source_manifest([pkg]))
   (directory/'go.sum').write_text('changed lock')
   self.assertNotEqual(boundary.canonical_hash(boundary.source_manifest([pkg])),before)
   before=boundary.canonical_hash(boundary.source_manifest([pkg]))
   (directory/'main.go').write_text('package changed')
   self.assertNotEqual(boundary.canonical_hash(boundary.source_manifest([pkg])),before)

 def test_import_root_path(self):
  graph={'main':['http','direct'],'http':['tls'],'tls':['rsa'],'direct':['rsa']}
  self.assertEqual(boundary.shortest_path(graph,'main','rsa'),['main','direct','rsa'])

 @unittest.skipUnless(shutil.which('go'),'Go is required for actual stripped binary test')
 def test_actual_stripped_executable_has_no_acceptable_symbols(self):
  with tempfile.TemporaryDirectory() as temp:
   directory=Path(temp);source=directory/'main.go';source.write_text('package main\nfunc main() { println("fixture") }\n')
   binary=directory/'stripped'
   subprocess.run(['go','build','-ldflags=-s -w','-o',str(binary),str(source)],check=True,capture_output=True)
   result=subprocess.run(['go','tool','nm',str(binary)],capture_output=True,text=True)
   if result.returncode==0:
    with self.assertRaises(ValueError):boundary.parse_symbols(result.stdout)
   else:self.assertNotEqual(result.returncode,0)


if __name__=='__main__':unittest.main()
