module github.com/dytallix/root-authorization

go 1.22.0

require github.com/cloudflare/circl v1.6.3

require (
	golang.org/x/crypto v0.30.0 // indirect
	golang.org/x/sys v0.28.0
)

require dytallix.local/consensus/owner-guard v0.0.0

replace dytallix.local/consensus/owner-guard => ../owner-guard
