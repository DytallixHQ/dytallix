package enginepqc

import (
	"net"
	"strconv"
)

// privateEndpoint accepts only a canonical IP literal on a private network.
// This is a staging boundary, not an approved production topology.
func privateEndpoint(address string) bool {
	host, port, err := net.SplitHostPort(address)
	if err != nil {
		return false
	}
	ip := net.ParseIP(host)
	n, err := strconv.Atoi(port)
	return err == nil && n >= 1024 && n <= 65535 && ip != nil && ip.IsPrivate() && net.JoinHostPort(ip.String(), strconv.Itoa(n)) == address
}

// explicitPeerEndpoint accepts one canonical, routable IP literal and port.
// It does not decide which addresses production may use. That allowlist must
// come from the approved production configuration and full-key peer pins.
func explicitPeerEndpoint(address string) bool {
	host, port, err := net.SplitHostPort(address)
	if err != nil {
		return false
	}
	ip := net.ParseIP(host)
	n, err := strconv.Atoi(port)
	return err == nil && n >= 1024 && n <= 65535 && ip != nil && ip.IsGlobalUnicast() && net.JoinHostPort(ip.String(), strconv.Itoa(n)) == address
}

func endpointIP(address string) net.IP {
	host, _, err := net.SplitHostPort(address)
	if err != nil {
		return nil
	}
	return net.ParseIP(host)
}
