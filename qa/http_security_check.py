"""Adversarial REST framing, connection budget, deadline and client response limits."""
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time

BINARY = str(Path(sys.argv[1]).resolve())
TOKEN = 'http-security-test-token-00000000'

with tempfile.TemporaryDirectory(prefix='rhyven-http-security-') as folder:
    env = dict(os.environ, RHYVEN_SERVE_TOKEN=TOKEN)
    server = subprocess.Popen([BINARY, '--workspace', folder, 'serve', '--port', '0'], env=env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    sockets = []
    try:
        startup = json.loads(server.stderr.readline())
        port = int(startup['address'].rsplit(':', 1)[1])

        def send(data):
            with socket.create_connection(('127.0.0.1', port), timeout=3) as conn:
                conn.sendall(data)
                return conn.makefile('rb').readline()

        auth = f'Authorization: Bearer {TOKEN}\r\n'.encode()
        assert b'400' in send(b'GET /categories HTTP/1.1\r\nX-Long: ' + b'x' * 40000 + b'\r\n\r\n')
        assert b'400' in send(b'GET /categories HTTP/1.1\r\n' + auth + auth + b'\r\n')
        assert b'400' in send(b'POST /categories HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\n\r\n')
        assert b'401' in send(b'GET /categories HTTP/1.1\r\n\r\n')
        assert b'200' in send(b'GET /categories HTTP/1.1\r\n' + auth + b'\r\n')
        time.sleep(.1)
        started = time.monotonic()
        for _ in range(32):
            conn = socket.create_connection(('127.0.0.1', port), timeout=2)
            conn.sendall(b'GET /categories HTTP/1.1\r\nX-Slow: ')
            sockets.append(conn)
        time.sleep(.1)
        with socket.create_connection(('127.0.0.1', port), timeout=2) as conn:
            assert conn.recv(1) == b'', 'Excess connection was not dropped'
        # Send occasional bytes: the deadline must not restart on each read.
        while time.monotonic() - started < 31:
            for conn in sockets:
                try:
                    conn.sendall(b'x')
                except OSError:
                    pass
            time.sleep(1)
        assert b'200' in send(b'GET /categories HTTP/1.1\r\n' + auth + b'\r\n'), 'Request slots did not recover after deadline'
    finally:
        for conn in sockets:
            conn.close()
        server.terminate()
        server.wait(timeout=5)

    class Oversized(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header('Content-Length', '8388608')
            self.end_headers()
            try:
                for _ in range(128):
                    self.wfile.write(b'x' * 65536)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def log_message(self, *_):
            pass

    fake = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Oversized)
    thread = threading.Thread(target=fake.serve_forever, daemon=True)
    thread.start()
    try:
        messages = [
            {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2025-11-25', 'capabilities': {}, 'clientInfo': {'name': 'security-test', 'version': '1'}}},
            {'jsonrpc': '2.0', 'method': 'notifications/initialized', 'params': {}},
            {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call', 'params': {'name': 'rhyven_categories', 'arguments': {}}},
        ]
        result = subprocess.run([BINARY, '--workspace', folder, 'mcp', '--server', f'http://127.0.0.1:{fake.server_port}'],
            input=''.join(json.dumps(m) + '\n' for m in messages), text=True, capture_output=True, env=env, timeout=10)
        assert 'exceeds 1 MiB' in result.stdout, (result.stdout, result.stderr)
    finally:
        fake.shutdown()
        fake.server_close()
        thread.join(timeout=5)
print('PASS: bounded HTTP headers/responses, duplicate/framing rejection, 32-connection cap, drip-feed deadline and recovery')
