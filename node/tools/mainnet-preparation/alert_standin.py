#!/usr/bin/env python3
"""A stand-in for the alerting service in staging runs (E05; monitoring v1,
M5): an HTTP server on a loopback port that takes the monitor job's
heartbeats, alerts and escalations (POST /heartbeat, /alert, /escalation)
and records each, with the time it arrived, as one line of ROOT/events.jsonl.
Like the real service, it alerts on its own when a host's heartbeat stops
for the approved time (monitoring down, 3 minutes) and resolves when it
returns. Standard library only. Never for a real service.

  alert_standin.py --root DIR --port PORT [--missing-seconds 180]
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
import time

ALERT_SCHEMA = 'dytallix.alert.v1'
PATHS = {'/heartbeat': 'heartbeat', '/alert': 'alert', '/escalation': 'escalation'}
MAX_BODY = 65536


class Service:
    """The events file and each host's last heartbeat."""

    def __init__(self, root, missing_seconds, clock=time.time):
        self.path = Path(root) / 'events.jsonl'
        self.missing, self.clock = missing_seconds, clock
        self.lock = threading.Lock()
        self.hosts = {}  # host -> {'last': time, 'down': bool, 'heartbeat': last payload}

    def record(self, to, body):
        with open(self.path, 'a') as file:
            file.write(json.dumps({'received': round(self.clock(), 3), 'to': to, 'body': body},
                                  separators=(',', ':')) + '\n')

    def own_alert(self, host, firing, missing):
        heartbeat = self.hosts[host]['heartbeat']
        self.record('service', {
            'schema': ALERT_SCHEMA, 'kind': 'change', 'status': 'firing' if firing else 'resolved', 'severity': 1,
            'condition': 'monitoring_down', 'host': host, 'role': heartbeat.get('role'),
            'chain_id': heartbeat.get('chain_id'), 'value': round(missing), 'threshold': self.missing,
            'time': round(self.clock())})

    def receive(self, to, body):
        with self.lock:
            self.record(to, body)
            if to != 'heartbeat':
                return
            host = body.get('host')
            seen = self.hosts.get(host)
            if seen and seen['down']:
                missing = self.clock() - seen['last']
                seen['down'] = False
                seen['heartbeat'] = body
                self.own_alert(host, False, missing)
            self.hosts[host] = {'last': self.clock(), 'down': False, 'heartbeat': body}

    def check(self):
        """Alerts once for each host whose heartbeat stopped."""
        with self.lock:
            now = self.clock()
            for host, seen in self.hosts.items():
                if not seen['down'] and now - seen['last'] >= self.missing:
                    seen['down'] = True
                    self.own_alert(host, True, now - seen['last'])


def handler(service):
    class Handler(BaseHTTPRequestHandler):
        def answer(self, code):
            self.send_response(code)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def do_POST(self):
            to = PATHS.get(self.path)
            length = int(self.headers.get('Content-Length', '-1'))
            if to is None:
                return self.answer(404)
            if not 0 < length <= MAX_BODY:
                return self.answer(411)
            try:
                body = json.loads(self.rfile.read(length))
            except ValueError:
                return self.answer(400)
            if type(body) is not dict or not isinstance(body.get('host'), str):
                return self.answer(400)
            service.receive(to, body)
            self.answer(200)

        def log_message(self, format, *args):
            print(f'{self.command} {self.path} {args[1] if len(args) > 1 else ""}', flush=True)

    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--port', type=int, required=True)
    parser.add_argument('--missing-seconds', type=int, default=180)
    args = parser.parse_args()
    args.root.mkdir(parents=True, exist_ok=True)
    service = Service(args.root, args.missing_seconds)
    server = ThreadingHTTPServer(('127.0.0.1', args.port), handler(service))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    print(f'stand-in alerting service on 127.0.0.1:{args.port}, {args.root}', flush=True)
    while True:
        time.sleep(5)
        service.check()


if __name__ == '__main__':
    raise SystemExit(main())
