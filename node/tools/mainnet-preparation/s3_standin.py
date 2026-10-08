#!/usr/bin/env python3
"""A stand-in for the off-host store in staging runs (E05; disaster recovery
v1, H4): an S3-shaped HTTP server on a loopback port that accepts only
signed PUTs (AWS Signature V4 from the expected key ID) and writes each
object under ROOT/BUCKET/KEY. It does not check the signature itself; the
backup job's curl must send one. Standard library only. Never for a real
store.

  s3_standin.py --root DIR --port PORT --access-key-id ID
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path, PurePosixPath
from urllib.parse import unquote, urlsplit

MAX_OBJECT_BYTES = 8 << 30


def handler(root, access_key_id):
    class Handler(BaseHTTPRequestHandler):
        # curl sends Expect: 100-continue for large uploads.
        protocol_version = 'HTTP/1.1'

        def refuse(self, code, reason):
            body = reason.encode()
            self.send_response(code)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_PUT(self):
            authorization = self.headers.get('Authorization', '')
            if not authorization.startswith(f'AWS4-HMAC-SHA256 Credential={access_key_id}/'):
                return self.refuse(403, 'unsigned request')
            path = PurePosixPath(unquote(urlsplit(self.path).path))
            parts = path.parts[1:]
            if len(parts) < 2 or any(p in ('.', '..') for p in parts):
                return self.refuse(400, 'bucket and key required')
            length = int(self.headers.get('Content-Length', '-1'))
            if not 0 <= length <= MAX_OBJECT_BYTES:
                return self.refuse(411, 'length required')
            target = Path(root).joinpath(*parts)
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
    parser.add_argument('--access-key-id', required=True)
    args = parser.parse_args()
    args.root.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer(('127.0.0.1', args.port), handler(args.root, args.access_key_id))
    print(f'stand-in store on 127.0.0.1:{args.port}, {args.root}', flush=True)
    server.serve_forever()


if __name__ == '__main__':
    raise SystemExit(main())
