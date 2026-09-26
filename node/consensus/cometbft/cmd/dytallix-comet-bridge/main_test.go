package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	abci "github.com/cometbft/cometbft/abci/types"
)

func TestBridgeChildHelper(t *testing.T) {
	mode := os.Getenv("DYTALLIX_BRIDGE_TEST_CHILD")
	if mode == "" {
		return
	}
	scan := bufio.NewScanner(os.Stdin)
	for scan.Scan() {
		var req struct {
			Method  string          `json:"method"`
			Payload json.RawMessage `json:"payload"`
		}
		if json.Unmarshal(scan.Bytes(), &req) != nil {
			os.Exit(2)
		}
		switch mode {
		case "trailing":
			fmt.Println(`{"ok":true,"result":{}} {}`)
		case "negative":
			fmt.Println(`{"ok":false,"error":"fixture rejection"}`)
		case "evidence":
			var payload map[string]json.RawMessage
			if json.Unmarshal(req.Payload, &payload) != nil {
				os.Exit(2)
			}
			if string(payload["evidence_max_age_blocks"]) != "7" || string(payload["evidence_max_age_seconds"]) != "3" || string(payload["evidence_max_age_nanos"]) != "5" {
				fmt.Println(`{"ok":false,"error":"engine evidence limits were not forwarded exactly"}`)
			} else {
				fmt.Println(`{"ok":true,"result":{"app_hash":"0000000000000000000000000000000000000000000000000000000000000000"}}`)
			}
		case "misbehavior":
			var payload map[string]json.RawMessage
			if json.Unmarshal(req.Payload, &payload) != nil {
				os.Exit(2)
			}
			if string(payload["misbehavior"]) != os.Getenv("DYTALLIX_BRIDGE_TEST_FACTS") {
				fmt.Println(`{"ok":false,"error":"misbehavior facts changed"}`)
			} else {
				fmt.Println(`{"ok":true,"result":{"txs":[],"accept":true,"app_hash":"0000000000000000000000000000000000000000000000000000000000000000","tx_results":[]}}`)
			}
		case "updates":
			fmt.Println(os.Getenv("DYTALLIX_BRIDGE_TEST_RESPONSE"))
		case "count":
			fmt.Println(`{"ok":true,"result":{"app_hash":"0000000000000000000000000000000000000000000000000000000000000000","tx_results":[]}}`)
		default:
			fmt.Printf("{\"ok\":true,\"result\":%s}\n", req.Payload)
		}
	}
	os.Exit(0)
}

func testChild(t *testing.T, mode string) *child {
	t.Helper()
	t.Setenv("DYTALLIX_BRIDGE_TEST_CHILD", mode)
	ctx, cancel := context.WithCancel(context.Background())
	c, err := startChild(ctx, []string{os.Args[0], "-test.run=^TestBridgeChildHelper$"})
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	t.Cleanup(func() {
		cancel()
		select {
		case <-c.done:
		case <-time.After(5 * time.Second):
			t.Error("child did not exit")
		}
	})
	return c
}

func TestChildRoundTripAndApplicationError(t *testing.T) {
	c := testChild(t, "echo")
	var result map[string]int
	if err := c.call(context.Background(), "fixture", map[string]int{"height": 7}, &result); err != nil {
		t.Fatal(err)
	}
	if result["height"] != 7 {
		t.Fatal(result)
	}
}

func TestChildRejectsTrailingJSONAndPoisonsStream(t *testing.T) {
	c := testChild(t, "trailing")
	var result struct{}
	if err := c.call(context.Background(), "fixture", struct{}{}, &result); err == nil {
		t.Fatal("accepted trailing JSON")
	}
	if c.broken == nil {
		t.Fatal("invalid stream remained usable")
	}
	if err := c.call(context.Background(), "fixture", struct{}{}, &result); err == nil {
		t.Fatal("reused invalid stream")
	}
}

func TestApplicationRejectionDoesNotPoisonWellFormedStream(t *testing.T) {
	c := testChild(t, "negative")
	var result struct{}
	if err := c.call(context.Background(), "fixture", struct{}{}, &result); err == nil {
		t.Fatal("ignored application rejection")
	}
	if c.broken != nil {
		t.Fatal("well-formed rejection poisoned stream")
	}
}

func TestFinalizeRequiresOneResultPerTransaction(t *testing.T) {
	a := application{child: testChild(t, "count")}
	_, err := a.FinalizeBlock(context.Background(), &abci.RequestFinalizeBlock{Height: 1, Time: time.Unix(1, 0), Hash: make([]byte, 32), Txs: [][]byte{[]byte("tx")}})
	if err == nil {
		t.Fatal("accepted missing transaction result")
	}
}

func TestSocketRequiresPrivateUnusedUnixPath(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "app.sock")
	if _, err := privateSocketPath("tcp://127.0.0.1:26658"); err == nil {
		t.Fatal("accepted TCP")
	}
	if _, err := privateSocketPath("unix://" + path); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := privateSocketPath("unix://" + path); err == nil {
		t.Fatal("accepted accessible directory")
	}
	if err := os.Chmod(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("existing"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := privateSocketPath("unix://" + path); err == nil {
		t.Fatal("accepted existing endpoint")
	}
}

func TestUnsupportedFeaturesFailClosed(t *testing.T) {
	a := application{}
	ctx := context.Background()
	if _, err := a.InsertTx(ctx, &abci.RequestInsertTx{}); err == nil {
		t.Fatal("application mempool enabled")
	}
	if _, err := a.ExtendVote(ctx, &abci.RequestExtendVote{}); err == nil {
		t.Fatal("vote extensions enabled")
	}
	offer, err := a.OfferSnapshot(ctx, &abci.RequestOfferSnapshot{})
	if err != nil || offer.Result != abci.ResponseOfferSnapshot_REJECT {
		t.Fatal("snapshot accepted")
	}
	_, err = a.InitChain(ctx, &abci.RequestInitChain{InitialHeight: 1})
	if err == nil {
		t.Fatal("missing PQC consensus parameters accepted")
	}
}
