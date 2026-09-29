/*
Package privval provides the file implementation of types.PrivValidator.

# FilePV

FilePV uses one file for the ML-DSA-65 private key and another to store the
last signing state.

The remote signer (SignerListenerEndpoint, SignerDialerEndpoint, SignerClient
and the socket and Noise transports) was removed from this fork (Dytallix
E04 gap 20): it used classical key exchange and signatures, and the
validator signs only with its local file key.
*/
package privval
