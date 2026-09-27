#!/usr/bin/env python3
"""Serve only the local website directory. No runtime or registry access."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from security_headers import HEADERS

class Handler(SimpleHTTPRequestHandler):
    def end_headers(self):
        for name, value in HEADERS.items():
            self.send_header(name, value)
        self.send_header('Cache-Control', 'public, no-cache, no-transform')
        super().end_headers()

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=5173)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    try:
        server = ThreadingHTTPServer(('127.0.0.1', args.port), partial(Handler, directory=str(root)))
    except OSError as error:
        parser.exit(1, f'Could not start local preview: {error}\nTry --port 5174 if the port is in use.\n')
    print(f'Rhyven website: http://localhost:{args.port}\nPress Ctrl+C to stop.', flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
