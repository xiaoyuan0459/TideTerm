"""Loopback-only synthetic Engine API. Never proxy or read user Docker data."""
import argparse
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

parser = argparse.ArgumentParser()
parser.add_argument('--directory', required=True)
args = parser.parse_args()
root = Path(args.directory).resolve()
root.mkdir(parents=True, exist_ok=True)
lock = threading.Lock()
counts = dict(ping=0, lists=0, forbidden=0, bad_query=0)

def write(name, value):
    temporary = root / (name + '.tmp')
    temporary.write_text(json.dumps(value), encoding='utf-8')
    temporary.replace(root / name)

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Api-Version', '1.47')
        self.send_header('Docker-Experimental', 'false')
        self.end_headers()
        if self.command != 'HEAD':
            self.wfile.write(data)

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        parts = urlsplit(self.path)
        with lock:
            if parts.path == '/_ping' and self.command in ('GET', 'HEAD'):
                counts['ping'] += 1
                write('stats.json', counts)
                self.respond(200, 'OK')
                return
            if parts.path.endswith('/containers/json') and self.command == 'GET':
                query = parse_qs(parts.query)
                counts['lists'] += 1
                if query.get('all') not in (['1'], ['true']) or query.get('limit') != ['33'] or query.get('size') == ['1']:
                    counts['bad_query'] += 1
                write('stats.json', counts)
                state = json.loads((root / 'state.json').read_text(encoding='utf-8'))
                if state.get('fail'):
                    self.respond(503, {'message': 'synthetic failure'})
                else:
                    count = state.get('count', 2)
                    names = ['ui_alpha', 'ui_beta']
                    values = [dict(Id=f'{i+1:064x}', Names=['/' + ('ui_alpha\0hidden' if state.get('nul') and i == 0 else ('x' * 250) if state.get('long') else names[i] if i < 2 else f'ui_{i:02}')],
                                   Image='fixture:synthetic', ImageID='0'*64, Command='synthetic', Created=1,
                                   State='running', Status='synthetic', Ports=[], Labels={}, HostConfig={'NetworkMode': 'default'},
                                   NetworkSettings={'Networks': {}}, Mounts=[]) for i in range(count)]
                    self.respond(200, values)
                return
            counts['forbidden'] += 1
            write('stats.json', counts)
            self.respond(405, {'message': 'fixture permits ping and bounded list only'})

    do_POST = do_GET
    do_DELETE = do_GET
    do_PUT = do_GET

write('state.json', dict(count=2))
write('stats.json', counts)
server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
write('ready.json', dict(port=server.server_port))
server.serve_forever()
