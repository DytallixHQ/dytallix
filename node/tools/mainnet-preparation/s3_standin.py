#!/usr/bin/env python3
"""A stand-in for the off-host store in staging runs (E05; disaster recovery
v1, H4): an S3-shaped HTTP server on a loopback port that accepts only
signed PUTs (AWS Signature V4 from an expected key ID) and writes each
object under ROOT/BUCKET/KEY, and, for the staging agent's command store
(T2), signed GETs from the read key ID. The founder key ID (T2's real-host
harness, staging_founder.py) may also PUT, GET and LIST (ListObjectsV2), as
the founder's own key does on the real store. It does not check the
signature itself; the jobs' curl must send one. Standard library only.
Never for a real store.

  s3_standin.py --root DIR --port PORT --access-key-id ID [--access-key-id ID ...] [--read-key-id ID]
                [--founder-key-id ID] [--bind ADDRESS]
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path, PurePosixPath
from urllib.parse import parse_qs, unquote, urlsplit
from xml.sax.saxutils import escape

MAX_OBJECT_BYTES = 8 << 30


def signed_by(headers, key_ids):
    authorization = headers.get('Authorization', '')
    return any(authorization.startswith(f'AWS4-HMAC-SHA256 Credential={key}/') for key in key_ids)


def object_path(root, raw_path):
    parts = PurePosixPath(unquote(urlsplit(raw_path).path)).parts[1:]
    if len(parts) < 2 or any(p in ('.', '..') for p in parts):
        return None
    return Path(root).joinpath(*parts)


def listing(root, raw_path):
    """ListObjectsV2 for one bucket: every key under the prefix, in one page."""
    split = urlsplit(raw_path)
    parts = PurePosixPath(unquote(split.path)).parts[1:]
    if len(parts) != 1 or parts[0] in ('.', '..'):
        return None
    prefix = parse_qs(split.query).get('prefix', [''])[0]
    bucket = Path(root) / parts[0]
    keys = sorted(p.relative_to(bucket).as_posix() for p in bucket.rglob('*') if p.is_file()) if bucket.is_dir() else []
    contents = ''.join(f'<Contents><Key>{escape(k)}</Key></Contents>' for k in keys if k.startswith(prefix))
    return ('<?xml version="1.0" encoding="UTF-8"?>'
            '<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
            f'<Name>{escape(parts[0])}</Name><Prefix>{escape(prefix)}</Prefix>'
            f'<IsTruncated>false</IsTruncated>{contents}</ListBucketResult>').encode()


def handler(root, access_key_ids, read_key_id=None, founder_key_id=None):
    class Handler(BaseHTTPRequestHandler):
        # curl sends Expect: 100-continue for large uploads.
        protocol_version = 'HTTP/1.1'

        def refuse(self, code, reason):
            body = reason.encode()
            self.send_response(code)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            readers = [k for k in (read_key_id, founder_key_id) if k]
            if not readers or not signed_by(self.headers, readers):
                return self.refuse(403, 'unsigned request')
            if 'list-type=2' in urlsplit(self.path).query:
                if not (founder_key_id and signed_by(self.headers, [founder_key_id])):
                    return self.refuse(403, 'only the founder key lists')
                body = listing(root, self.path)
                if body is None:
                    return self.refuse(400, 'a bucket is required')
                self.send_response(200)
                self.send_header('Content-Type', 'application/xml')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                return self.wfile.write(body)
            target = object_path(root, self.path)
            if target is None:
                return self.refuse(400, 'bucket and key required')
            if not target.is_file():
                return self.refuse(404, 'no such key')
            body = target.read_bytes()
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_PUT(self):
            if not signed_by(self.headers, access_key_ids + ([founder_key_id] if founder_key_id else [])):
                return self.refuse(403, 'unsigned request')
            target = object_path(root, self.path)
            if target is None:
                return self.refuse(400, 'bucket and key required')
            length = int(self.headers.get('Content-Length', '-1'))
            if not 0 <= length <= MAX_OBJECT_BYTES:
                return self.refuse(411, 'length required')
            if target.exists():
                return self.refuse(409, 'objects are never replaced')
            target.parent.mkdir(parents=True, exist_ok=True)
            with open(target, 'xb') as file:
                remaining = length
                while remaining:
                    block = self.rfile.read(min(remaining, 1 << 20))
                    if not block:
                        break
                    file.write(block)
                    remaining -= len(block)
            if remaining:
                os.unlink(target)
                return self.refuse(400, 'short body')
            self.send_response(200)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def log_message(self, format, *args):
            print(f'{self.command} {self.path} {args[1] if len(args) > 1 else ""}', flush=True)

    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--port', type=int, required=True)
    parser.add_argument('--access-key-id', action='append', required=True)
    parser.add_argument('--read-key-id')
    parser.add_argument('--founder-key-id')
    parser.add_argument('--bind', default='127.0.0.1')
    args = parser.parse_args()
    args.root.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer((args.bind, args.port),
                                 handler(args.root, args.access_key_id, args.read_key_id, args.founder_key_id))
    print(f'stand-in store on {args.bind}:{args.port}, {args.root}', flush=True)
    server.serve_forever()


if __name__ == '__main__':
    raise SystemExit(main())
