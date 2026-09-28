// Command dytallix-fixture-sign signs a test artifact with a disposable
// SLH-DSA-SHAKE-256s key for the node's signed-fixture tests (E04 gap 11).
//
// DEVELOPMENT ONLY. Each key is derived from one public byte (--fixture-key),
// so every key it can produce is known to everyone and must never be trusted
// by a real chain. The private key is never written; the output holds the
// public key and the signed verification request.
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"github.com/cloudflare/circl/sign/slhdsa"
	root "github.com/dytallix/root-authorization"
	"os"
)

func main() {
	artifactPath := flag.String("artifact", "", "public artifact path")
	output := flag.String("output", "", "public output path")
	chain := flag.String("chain", "emergency-verifier-development", "development chain")
	sequence := flag.Uint64("sequence", 1, "sequence")
	height := flag.Uint64("height", 10, "current and default validity height")
	before := flag.Uint64("not-before", 0, "first valid height; zero uses height")
	after := flag.Uint64("not-after", 0, "last valid height; zero uses height")
	actionName := flag.String("action", "emergency", "trusted root action: emergency or upgrade")
	seed := flag.Uint("fixture-key", 17, "disposable test key discriminator, 1..255")
	flag.Parse()
	if *artifactPath == "" || *output == "" || *seed == 0 || *seed > 255 {
		panic("explicit fixture paths and bounded test discriminator required")
	}
	action := root.Action(*actionName)
	if action != root.Emergency && action != root.Upgrade {
		panic("only emergency or upgrade fixture actions allowed")
	}
	if *before == 0 {
		*before = *height
	}
	if *after == 0 {
		*after = *height
	}
	if *before > *height || *after < *height {
		panic("current height must lie in fixture validity window")
	}
	artifact, err := os.ReadFile(*artifactPath)
	must(err)
	if len(artifact) > 65536 {
		panic("test artifact exceeds bound")
	}
	pub, priv, err := slhdsa.GenerateKey(bytes.NewReader(bytes.Repeat([]byte{byte(*seed)}, 96)), slhdsa.SHAKE_256s)
	must(err)
	public, err := pub.MarshalBinary()
	must(err)
	private, err := priv.MarshalBinary()
	must(err)
	defer func() {
		for i := range private {
			private[i] = 0
		}
	}()
	envelope := root.Envelope{Version: root.Version, Profile: root.Profile, ChainID: *chain, Action: action, Sequence: *sequence, NotBeforeHeight: *before, NotAfterHeight: *after, ArtifactDigest: root.DigestArtifact(artifact)}
	policy := root.Policy{TrustedPublicKey: public, ChainID: *chain, Action: action, ExpectedSequence: *sequence, CurrentHeight: *height, ExpectedArtifactDigest: root.DigestArtifact(artifact)}
	signature, err := root.SignForPolicy(envelope, private, policy)
	must(err)
	raw, err := json.Marshal(struct {
		PublicKey []byte
		Request   root.VerificationRequest
	}{public, root.VerificationRequest{Envelope: envelope, Signature: signature, Artifact: artifact}})
	must(err)
	must(os.WriteFile(*output, raw, 0600))
	fmt.Println("Disposable development signature written; no private key serialized")
}
func must(err error) {
	if err != nil {
		panic(err)
	}
}
