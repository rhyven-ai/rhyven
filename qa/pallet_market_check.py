"""Live, opt-in acceptance for the public text pallet in disposable local homes.

Requires explicit operator consent. The MCP test client relays that consent;
it is not a claim of verification inside Codex or another production harness.
"""
import argparse
import json
from pathlib import Path
import selectors
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary', type=Path)
parser.add_argument('--approve-test-source', action='store_true',
                    help='Operator approves the reviewed text pallet in disposable test homes')
args = parser.parse_args()
if not args.approve_test_source:
    parser.error('Explicit operator approval required: --approve-test-source')
binary = str(args.binary.resolve())
selector = 'rhyven-test/text-kit@0.1.0'
expected_sha = 'afa18d564761ae18b775fa30f371618fd1a9a7020b7cac785233d110ab17e414'
report = {'registry': 'rhyven-ai/pallet-text-test', 'pallet': selector,
          'client': 'minimal MCP test client; explicit operator consent', 'checks': []}

with tempfile.TemporaryDirectory(prefix='rhyven-pallet-market-') as tmp:
    root = Path(tmp)
    project = root / 'project'
    project.mkdir()
    home = root / 'home'
    base = [binary, '--home', str(home), '--collection', 'trial', '--project', str(project)]

    def cli(*cmd, prefix=None):
        result = subprocess.run((prefix or base) + list(cmd), text=True, capture_output=True, timeout=120)
        assert result.returncode == 0, result.stderr + result.stdout
        return json.loads(result.stdout)

    cli('registry-sync', report['registry'], '--anonymous')
    listing = cli('pallet', 'search', 'normalization')[0]
    assert listing['sha256'] == expected_sha
    assert cli('pallet', 'list') == []
    before = cli('call', 'rhyven_categories', '{}')
    report['checks'].append('anonymous GitHub index sync and exact release hash')

    with tempfile.TemporaryFile(mode='w+t') as errors:
        proc = subprocess.Popen(base + ['mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=errors, text=True, bufsize=1)
        poll = selectors.DefaultSelector()
        poll.register(proc.stdout, selectors.EVENT_READ)
        next_id = 0

        def send(value):
            proc.stdin.write(json.dumps(value) + '\n')
            proc.stdin.flush()

        def receive():
            assert poll.select(timeout=90), 'MCP response timed out'
            line = proc.stdout.readline()
            assert line, 'MCP server closed unexpectedly'
            return json.loads(line)

        def rpc(method, params):
            global next_id
            next_id += 1
            send({'jsonrpc': '2.0', 'id': next_id, 'method': method, 'params': params})
            return receive()

        def tool(function, values):
            return rpc('tools/call', {'name': 'rhyven_call', 'arguments': {
                'category': 'rhyven/marketplace', 'function': function, 'args': values}})

        def content(response):
            assert not response.get('error'), response
            assert not response['result'].get('isError'), response
            return json.loads(response['result']['content'][0]['text'])

        try:
            rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {'elicitation': {}},
                               'clientInfo': {'name': 'pallet-acceptance', 'version': '1'}})
            send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
            tools = rpc('tools/list', {})['result']['tools']
            assert {t['name'] for t in tools} == {'rhyven_categories', 'rhyven_describe', 'rhyven_call'}
            contract = content(rpc('tools/call', {'name': 'rhyven_describe', 'arguments': {'category': 'rhyven/marketplace', 'index': True}}))
            assert 'action_prepare_pallet' in json.dumps(contract)
            assert content(tool('action_pallet_search', {'query': 'normalization'}))[0]['sha256'] == expected_sha
            for scope in ['workspace', 'global']:
                request = content(tool('action_prepare_pallet', {'selector': selector, 'scope': scope}))
                assert request['sha256'] == expected_sha
                assert request['permissions'] == []
                prompt = tool('action_apply', {'request_id': request['request_id']})
                assert prompt['method'] == 'elicitation/create', prompt
                assert expected_sha in prompt['params']['message']
                # Relay the operator's explicit authorization, never invent user consent.
                send({'jsonrpc': '2.0', 'id': prompt['id'], 'result': {
                    'action': 'accept', 'content': {'approve': True}}})
                result = content(receive())
                assert result['scope'] == scope and not result['installed_app']
                assert content(tool('action_apply', {'request_id': request['request_id']})) == result
                report['checks'].append(f'{scope}: MCP review, host elicitation, real asset download, idempotent retry')
                if scope == 'workspace':
                    other = [binary, '--home', str(home), '--collection', 'second']
                    assert cli('pallet', 'list', prefix=other) == []
            assert {p['scope'] for p in content(tool('action_pallet_list', {}))} == {'workspace', 'global'}
            contract = content(tool('action_pallet_describe', {'selector': selector, 'export': 'prepare_document'}))
            assert 'prepare_document' in json.dumps(contract) and 'files' not in contract
        finally:
            proc.stdin.close()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
            poll.close()

    other = [binary, '--home', str(home), '--collection', 'second', '--actor', 'second-agent']
    assert cli('pallet', 'list', prefix=other)[0]['scope'] == 'global'
    for prefix in [base, other]:
        result = cli('pallet', 'run', selector, 'prepare_document', '--args',
                     '{"title":"  Customer   Release Notes!  "}', '--allow-host', prefix=prefix)
        assert result == {'title': 'Customer Release Notes!', 'slug': 'customer-release-notes'}
    exported = root / 'source'
    cli('pallet', 'export', selector, '--dir', str(exported))
    subprocess.run(['python3', '-c', "from textkit import normalize, slug; "
                    "assert normalize({'text':' A\\tB\\nC '}) == {'text':'A B C'}; "
                    "assert slug({'text':'Release 2026.10!'}) == {'slug':'release-2026-10'}"],
                   cwd=exported, check=True, timeout=10)
    assert cli('call', 'rhyven_categories', '{}') == before
    report['checks'].append('second-agent global reuse and standalone Python imports; no app registration')
    report['passed'] = True
print(json.dumps(report, indent=2))
