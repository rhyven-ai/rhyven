"""Exercise knowledge transfer between real MCP and authenticated REST endpoints."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from universal_market_check import Client

binary = str(Path(sys.argv[1]).resolve())
package = Path(__file__).resolve().parents[1] / 'catalog/project-knowledge.json'

with tempfile.TemporaryDirectory(prefix='rhyven-knowledge-merge-') as temp:
    source, destination = Path(temp) / 'source', Path(temp) / 'destination'
    for root in (source, destination):
        subprocess.run([binary, '--workspace', str(root), 'install', str(package),
                        '--accept-permissions'], check=True, stdout=subprocess.DEVNULL)
    client = Client(source)
    def mcp(client, function, args):
        result = client.request('tools/call', {'name': 'rhyven_call', 'arguments': {
            'category': 'rhyven/project-knowledge', 'function': function, 'args': args}})
        assert not result.get('isError'), result
        return json.loads(result['content'][0]['text'])

    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    token = 'merge-acceptance-token-0123456789'
    env = dict(os.environ, RHYVEN_SERVE_TOKEN=token)
    server = subprocess.Popen([binary, '--workspace', str(destination), 'serve', '--port', str(port)],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    remote = None
    try:
        for _ in range(100):
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1):
                    break
            except OSError:
                assert server.poll() is None, server.stderr.read()
                time.sleep(.05)
        def http(function, args):
            request = urllib.request.Request(
                f'http://127.0.0.1:{port}/categories/rhyven/project-knowledge/functions/{function}',
                data=json.dumps(args).encode(), headers={'Authorization': 'Bearer ' + token,
                                                       'Content-Type': 'application/json'})
            with urllib.request.urlopen(request) as response:
                return json.load(response)
        base = mcp(client, 'object_note_create', {'data': {'title': 'Old', 'body': 'Original'}})
        mcp(client, 'object_note_create', {'data': {
            'title': 'New', 'body': 'Updated', 'supersedes': base['id']}})
        bundle = mcp(client, 'object_note_export', {'current_only': True})['bundle']
        preview = http('object_note_merge_preview', {'bundle': bundle})
        assert preview['new_records'] == 2
        applied = http('object_note_merge_apply', {'bundle': bundle, 'preview_token': preview['preview_token']})
        assert applied['applied']
        remote = Client(destination, server=f'http://127.0.0.1:{port}', env=env)
        query = {'current_only': True, 'any_of': [{'title': {'icontains': 'NEW'}}], 'select': ['title']}
        assert mcp(remote, 'object_note_query', query) == http('object_note_query', query)
        assert http('object_note_query', query)['items'][0]['data'] == {'title': 'New'}
        assert mcp(remote, 'object_note_merge_preview', {'bundle': bundle})['new_records'] == 0
        returned = mcp(remote, 'object_note_export', {})['bundle']
        assert mcp(client, 'object_note_merge_preview', {'bundle': returned})['new_records'] == 0
    finally:
        if remote:
            remote.close()
        client.close()
        server.terminate()
        server.wait(timeout=5)
print('PASS: local MCP export, REST preview/apply, remote MCP discovery/query parity, repeat import and round-trip identity')
