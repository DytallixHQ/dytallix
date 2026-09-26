package node

import "context"

// Node lifecycle requires shutdown behavior, not an HTTP implementation.
type auxiliaryServer interface{ Shutdown(context.Context) error }
