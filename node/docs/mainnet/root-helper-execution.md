# Development root helper execution

The trusted development root configuration accepts an optional `helper_scratch_path`.
Omit it, or use null, to retain the existing system temporary directory behavior.
An explicit path must name an existing absolute directory with no group or other permissions.
The leaf must not be a symlink. The directory must belong to the executing service user.
The caller must control the directory ancestry and prevent concurrent path replacement.
These pathname checks do not establish safety against a hostile process with the same user identity.

The application creates a random private child directory with mode 0700.
It copies the hash-verified helper into that directory and executes the copy with mode 0500.
It retains the exact helper digest, input/output bounds, cleared child environment and deadline.
It removes the child directory on return. Cleanup is best effort; filesystem failures can leave the private snapshot.
The application does not create or change the permissions or mount flags of the configured parent.

The selected mount must permit execution. Systemd `PrivateTmp` does not override a noexec mount.
Service read/write path restrictions must permit the selected private directory.
The installation directory and the private execution directory have separate purposes.
No production path, mount policy, service change or root action is selected by this option.

An unsuccessful helper exit is reported before an input-pipe error.
Spawn failures identify the verified snapshot execution step and its underlying operating-system error.
Helper stderr is not copied into application errors. Timeout and child-reaping behavior remain unchanged.

Qualification includes subprocess fixtures for default and explicit paths, private permissions,
leaf symlink refusal, absent/relative paths, exit-status reporting and snapshot cleanup.
The real development verifier test uses a private configured directory and checks atomic genesis,
restart and replay behavior. Subprocess fixtures do not establish cryptographic correctness.
Native local tests do not replace Linux artifact rebuilding or production host qualification.
