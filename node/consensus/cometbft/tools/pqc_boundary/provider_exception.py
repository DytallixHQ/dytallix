"""Fail-closed exception for one reviewed, disabled Go BoringCrypto stub selection."""

from pathlib import Path


BINARY_SHA256 = 'eca64913f4621d7fb8a59d88cdad2a298314d4c976aa916150099c914013d76b'
SOURCE_SHA256 = '2f4608c1fd30cfb9ff17f17c57c922269cbbf6e3980c6e61fdfdfde3db511c4c'
TOOLCHAIN_SHA256 = 'e40f3ea4a682f693c82f3c609ca1ae8ff36dd781f3f44d209be0b81248513df4'
TAGS = 'dytallix_pqc_only,dytallix_pqc_ipc'
PROVIDER = 'crypto/internal/boring'
SIG = PROVIDER + '/sig'
SELECTED = {
 PROVIDER: {
  'GoFiles': ['doc.go', 'notboring.go'], 'HFiles': ['goboringcrypto.h'],
  'IgnoredGoFiles': ['aes.go', 'boring.go', 'ecdh.go', 'ecdsa.go', 'hmac.go', 'rand.go', 'rsa.go', 'sha.go'],
  'IgnoredOtherFiles': ['div_test.c'], 'TestGoFiles': ['boring_test.go'],
  'Imports': ['crypto', 'crypto/cipher', SIG, 'hash'],
 },
 SIG: {
  'GoFiles': ['sig.go'], 'SFiles': ['sig_amd64.s'],
  'IgnoredOtherFiles': ['sig_other.s'], 'Imports': [],
 },
}
FILE_FIELDS = ('GoFiles', 'CgoFiles', 'IgnoredGoFiles', 'InvalidGoFiles',
               'CFiles', 'CXXFiles', 'MFiles', 'HFiles', 'FFiles', 'SFiles',
               'SwigFiles', 'SwigCXXFiles', 'SysoFiles', 'EmbedFiles',
               'TestGoFiles', 'XTestGoFiles', 'TestEmbedFiles',
               'XTestEmbedFiles', 'IgnoredOtherFiles')
EMBED_FIELDS = ('EmbedPatterns', 'TestEmbedPatterns', 'XTestEmbedPatterns')
CGO_FIELDS = ('CgoCFLAGS', 'CgoCPPFLAGS', 'CgoCXXFLAGS', 'CgoFFLAGS',
              'CgoLDFLAGS', 'CgoPkgConfig')
FILE_SHA256 = {
 'crypto/internal/boring/doc.go': '085f6c0eeefbae4baa9d90a6cd081d4b5f769551dec00cb8d4ddbbbd5b9353b0',
 'crypto/internal/boring/notboring.go': 'ca7890ca233a46d31b7d7d67d7391a97956f2b2aa66a882244ba5be98a75f88d',
 'crypto/internal/boring/goboringcrypto.h': 'd02b93dd188657550805418a74306addb0a96f1fc7a3346e91dc9cb9533610c0',
 'crypto/internal/boring/sig/sig.go': 'faed644e3071cddf01e07f209561daaa7fb6f54157ba417db60d942dbd80d523',
 'crypto/internal/boring/sig/sig_amd64.s': '0fab1084bb898eec15920788d6650434f5b49bdb354c21de57a2f586979e2435',
}


def require(condition, message):
 if not condition:
  raise ValueError(message)


def check_selected_packages(packages):
 """Reject new selected source categories, imports, and provider file drift."""
 require(set(packages) == {PROVIDER, SIG}, 'provider selection incomplete')
 for name, expected in SELECTED.items():
  actual = packages[name]
  require(not actual.get('Error') and not actual.get('DepsErrors') and not actual.get('Incomplete'),
          'provider dependency graph incomplete: ' + name)
  for field in FILE_FIELDS:
   require(actual.get(field, []) == expected.get(field, []), 'selected file list drift: ' + name + '/' + field)
  for field in EMBED_FIELDS + CGO_FIELDS:
   require(not actual.get(field), 'unreviewed source option: ' + name + '/' + field)
  for field, value in actual.items():
   if field.endswith('Files') and field not in FILE_FIELDS:
    require(not value, 'unknown nonempty file category: ' + name + '/' + field)
  require(actual.get('Imports', []) == expected['Imports'], 'provider import selection drift: ' + name)


def review(report, packages):
 """Permit only the exact pinned disabled stub inside a passing base inventory."""
 require(report['launch_status'] == 'NO_GO' and report['g35_status'] == 'NOT_GRANTED', 'gate status changed')
 require(report['errors'] == [], 'another boundary failure exists')
 require(report['binary']['sha256'] == BINARY_SHA256, 'binary drift')
 require(report['source']['sha256'] == SOURCE_SHA256, 'source or lock drift')
 require(report['toolchain']['sha256'] == TOOLCHAIN_SHA256, 'toolchain drift')
 require(report['source']['selected_build_tags'] == TAGS, 'source tag drift')
 require(report['rebuild']['requested'] and report['rebuild']['byte_identical'] and report['rebuild']['sha256'] == BINARY_SHA256,
         'byte-identical source binding absent')
 require(not report['prohibited_packages'] and not report['prohibited_symbols'], 'prohibited code found')
 require(not report['provider_review_symbols'], 'linked provider code found')
 require({p['package'] for p in report['provider_review_packages']} == {PROVIDER, SIG}, 'unexpected provider package')
 require(all(p['rules'] == ['provider_container_requires_review'] for p in report['provider_review_packages']),
         'unexpected provider rule')
 require(report['source']['graph'][PROVIDER] == SELECTED[PROVIDER]['Imports'] and report['source']['graph'][SIG] == [],
         'provider import graph drift')
 version = report['go_version_m']
 require(version.splitlines()[0].endswith('go1.26.0'), 'Go version drift')
 for setting in ('\tbuild\tCGO_ENABLED=0', '\tbuild\tGOOS=linux', '\tbuild\tGOARCH=amd64', '\tbuild\t-tags=' + TAGS):
  require(setting in version, 'build setting missing: ' + setting)
 manifest = report['source']['manifest']['files']
 for suffix, expected_hash in FILE_SHA256.items():
  hits = [(path, value) for path, value in manifest.items() if path.endswith('/src/' + suffix)]
  require(len(hits) == 1 and hits[0][1] == expected_hash, 'provider source pin drift: ' + suffix)
 require(not any(s.startswith('crypto/internal/boring') or s.startswith('_goboringcrypto')
                 for s in report['symbols']['all']), 'linked provider implementation symbol found')
 selected = {p['ImportPath']: p for p in packages if p['ImportPath'] in (PROVIDER, SIG)}
 check_selected_packages(selected)
 return {'status': 'RESOLVED_DISABLED_STUBS_FOR_PINNED_ENGINE',
         'scope': 'Only exact binary, Go source graph, toolchain, and selected provider files',
         'launch_status': 'NO_GO', 'g35_status': 'NOT_GRANTED'}
