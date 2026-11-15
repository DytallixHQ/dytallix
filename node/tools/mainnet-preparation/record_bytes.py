"""The bytes and times every evidence record in this folder is built from.

`canonical` is the one JSON encoding a record is digested over and `digest`
its SHA-256; a record's `sha256` is `record_digest` of the record without
it. Changing either changes every record's digest, so both stay exactly
what the tools wrote before they shared them. Standard library only: the
load and flood workers carry this file (load_workers.TOOLS).
"""

from datetime import datetime, timezone
import hashlib
import json
import time


def digest(data):
    """The SHA-256 of bytes, as lowercase hex."""
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    """Sorted keys, no spaces, UTF-8 bytes."""
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def record_digest(value):
    return digest(canonical(value))


def stamp():
    """Now, in UTC, to the second."""
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def iso_micro(seconds):
    """A Unix time in UTC, to the microsecond."""
    return datetime.fromtimestamp(seconds, timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')
