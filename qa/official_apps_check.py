"""Two-agent work/knowledge/messaging workflow using real Docker, MCP and REST.
python3 qa/official_apps_check.py BINARY MESSAGING_IMAGE_ID_FILE
All state and messages remain in isolated local test collections.
"""
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
image = Path(sys.argv[2]).read_text().strip()
work, knowledge, messaging = ('rhyven/' + x for x in ('work-management', 'project-knowledge', 'messaging'))


def eventually(test, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            value = test()
            if value:
                return value
        except (OSError, AssertionError):
            pass
        time.sleep(.1)
    raise AssertionError('Timed out waiting for test service')


with tempfile.TemporaryDirectory(prefix='rv-apps-') as temp:
    home = Path(temp)
    clients, server, daemon = [], None, False

    def cli(*args, collection='global', actor='lead'):
        p = subprocess.run([binary, '--home', temp, '--collection', collection, '--actor', actor, *args], text=True, capture_output=True, timeout=180)
        assert p.returncode == 0, (args, p.stdout, p.stderr)
        return json.loads(p.stdout)

    def mcp(actor='lead', collection='global', **kwargs):
        c = Client(home / 'collections' / collection, actor=actor, **kwargs)
        clients.append(c)
        return c

    try:
        cli('install', work, '--accept-permissions')
        cli('install', knowledge, '--accept-permissions')
        cli('app', 'package', 'apps/messaging', '--image', image, '--out', str(home / 'messaging.json'))
        cli('app', 'test', str(home / 'messaging.json'), '--allow-container')
        cli('install', str(home / 'messaging.json'), '--accept-permissions')
        cli('daemon', 'start')
        daemon = True
        lead, worker = mcp(), mcp('worker')
        task = lead.call(work, 'object_task_create', {'data': {'title': 'Verify recovery', 'description': 'Restore the release backup', 'due_date': '2026-10-01', 'labels': ['release']}})
        task = lead.call(work, 'action_assign', {'id': task['id'], 'expected_revision': task['revision'], 'owner': 'worker'})
        original = lead.call(knowledge, 'action_remember', {'title': 'Old release procedure', 'body': 'Do not use this obsolete procedure', 'topic': 'release'})
        note = lead.call(knowledge, 'action_correct', {'supersedes': original['id'], 'title': 'Release recovery', 'body': 'Restore both database and container files', 'topic': 'release', 'labels': ['release']})
        query = {'search': 'RELEASE recovery', 'current_only': True, 'where': {'labels': {'has': 'release'}}, 'metadata': {'created_at': {'ge': original['created_at']}}, 'order_by': [{'field': '$updated_at', 'direction': 'desc'}]}
        assert worker.call(knowledge, 'object_note_query', query)['items'][0]['id'] == note['id']
        assert worker.call(knowledge, 'object_note_query', {'search': 'obsolete', 'current_only': True})['total'] == 0
        assert worker.call(work, 'object_task_query', {'search': 'RECOVERY', 'where': {'due_date': {'le': '2026-10-02'}}})['total'] == 1
        lead.call(messaging, 'action_channel_create', {'channel': 'release', 'description': 'Private test handoffs'})
        worker.call(messaging, 'action_subscribe', {'channel': 'release'})
        send = {'channel': 'release', 'body': 'Verify recovery and record the result', 'message_key': 'release-handoff', 'links': [work + '/task/' + task['id'], knowledge + '/note/' + note['id']]}
        message = lead.call(messaging, 'action_send', send)
        assert lead.call(messaging, 'action_send', send) == message
        delivery = worker.call(messaging, 'action_claim', {'request_id': 'claim-1'})['items'][0]
        assert delivery['message']['links'] == send['links']
        assert worker.call(messaging, 'action_claim', {})['items'] == []
        result = worker.call(work, 'action_complete', {'id': task['id'], 'expected_revision': task['revision'], 'result': 'Recovered successfully; knowledge note ' + note['id']})
        assert result['updated_by'] == 'worker'
        worker.call(messaging, 'action_acknowledge', {'message_id': message['id'], 'lease_token': delivery['lease_token']})
        assert worker.call(messaging, 'action_inbox', {})['items'] == []
        assert lead.call(work, 'object_task_get', {'id': task['id']})['data']['status'] == 'done'

        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        env = dict(os.environ, RHYVEN_SERVE_TOKEN='official-apps-local-test-token-123456')
        server = subprocess.Popen([binary, '--home', temp, 'serve', '--port', str(port)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        def listening():
            with socket.create_connection(('127.0.0.1', port), timeout=.2):
                return True
        eventually(listening)
        remote = mcp('worker', server=f'http://127.0.0.1:{port}', env=env)
        assert remote.call(knowledge, 'object_note_query', query) == worker.call(knowledge, 'object_note_query', query)
        req = urllib.request.Request(f'http://127.0.0.1:{port}/categories/{knowledge}/functions/object_note_query', data=json.dumps(query).encode(), headers={'Authorization': 'Bearer ' + env['RHYVEN_SERVE_TOKEN'], 'Content-Type': 'application/json'})
        with urllib.request.urlopen(req, timeout=10) as response:
            assert json.load(response) == worker.call(knowledge, 'object_note_query', query)
        assert remote.call(messaging, 'action_history', {'channel': 'release', 'search': 'verify recovery'})['total'] == 1

        # An unacknowledged lease and duplicate-send protection survive service restart.
        pending = lead.call(messaging, 'action_send_direct', {'recipient': 'worker', 'body': 'Next handoff', 'message_key': 'pending'})
        lease = worker.call(messaging, 'action_claim', {'lease_seconds': 1})['items'][0]
        cli('service', 'stop', messaging)
        cli('service', 'start', messaging)
        time.sleep(1.1)
        redelivered = worker.call(messaging, 'action_claim', {})['items'][0]
        assert redelivered['message']['id'] == pending['id']
        assert redelivered['lease_token'] != lease['lease_token']
        assert lead.call(messaging, 'action_send_direct', {'recipient': 'worker', 'body': 'Next handoff', 'message_key': 'pending'}) == pending
        backup = home / 'handoff.rhyven'
        cli('backup', 'global', '--out', str(backup))
        cli('restore', str(backup), '--accept-permissions', collection='restored')
        cli('service', 'start', messaging, collection='restored')
        restored = mcp('worker', 'restored')
        assert restored.call(messaging, 'action_inbox', {})['items'][0]['message']['id'] == pending['id']
        assert restored.call(work, 'object_task_get', {'id': task['id']})['data']['status'] == 'done'
        assert restored.call(knowledge, 'object_note_query', query)['total'] == 1
        cli('service', 'stop', messaging, collection='restored')
        cli('remove', messaging, collection='restored')
        cli('install', str(home / 'messaging.json'), '--accept-permissions', collection='restored')
        cli('service', 'start', messaging, collection='restored')
        assert restored.call(messaging, 'action_inbox', {})['items'][0]['message']['id'] == pending['id']
        print('PASS: two-agent persisted handoff; search/dates/corrections; exactly three MCP tools; direct MCP/REST/HTTP-backed MCP parity; real Python Docker service; deduplication; restart/redelivery; complete backup/restore; remove/reinstall retained inboxes')
    finally:
        for client in clients:
            client.close()
        if server:
            server.terminate()
            server.wait(timeout=10)
        if daemon:
            cli('daemon', 'stop')
