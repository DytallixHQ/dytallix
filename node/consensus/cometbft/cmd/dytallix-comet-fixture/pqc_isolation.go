package main

import (
	"errors"
	"net"
	"strings"

	cfg "github.com/cometbft/cometbft/config"
)

// validateLegacyIsolation applies only to newly generated local fixtures.
// It is not a guard inside the upstream CometBFT executable.
func validateLegacyIsolation(c *cfg.Config) error {
	if c.PrivValidatorListenAddr != "" || c.P2P.LibP2PConfig.Enabled || c.P2P.ExternalAddress != "" || c.P2P.Seeds != "" || c.P2P.PexReactor || c.P2P.SeedMode || c.StateSync.Enable || c.RPC.GRPCListenAddress != "" || c.RPC.PprofListenAddress != "" {
		return errors.New("legacy fixture isolation requires remote signer, libp2p, discovery, state sync, and extra listeners disabled")
	}
	if !loopbackEndpoint(c.P2P.ListenAddress) || !loopbackEndpoint(c.RPC.ListenAddress) || !strings.HasPrefix(c.ProxyApp, "unix://") {
		return errors.New("legacy fixture requires loopback P2P/RPC and local Unix ABCI")
	}
	for _, peer := range strings.Split(c.P2P.PersistentPeers, ",") {
		if peer == "" {
			continue
		}
		_, address, ok := strings.Cut(peer, "@")
		if !ok || !loopbackEndpoint("tcp://"+address) {
			return errors.New("legacy fixture persistent peer must use a loopback IP")
		}
	}
	return nil
}
func loopbackEndpoint(endpoint string) bool {
	if !strings.HasPrefix(endpoint, "tcp://") {
		return false
	}
	host, port, e := net.SplitHostPort(strings.TrimPrefix(endpoint, "tcp://"))
	if e != nil || port == "" {
		return false
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}
