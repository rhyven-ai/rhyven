"""Local-only connector integration tests. No upstream software installed by Rhyven.
python3 qa/connector_check.py BINARY
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen
from universal_market_check import Client

binary = str(Path(sys.argv[1]).resolve())
seen = []
tool = {"name": "echo", "description": "Echo a message", "inputSchema": {
    "type": "object", "properties": {"message": {"type": "string"}}, "required": ["message"]}}

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def reply(self, status, value=None, **headers):
        body = b'' if value is None else json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        for k,v in headers.items(): self.send_header(k,v)
        self.end_headers()
        self.wfile.write(body)
    def do_DELETE(self):
        assert self.headers.get('Mcp-Session-Id') == 'local-test-session'
        self.reply(204)
    def do_GET(self):
        seen.append(('GET', self.path))
        if self.path.startswith('/api/items/'):
            self.reply(200, {'path': self.path})
        else: self.reply(404)
    def do_POST(self):
        data = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path == '/api/items':
            seen.append(('POST', self.path, data))
            self.reply(200, {'created': data})
            return
        assert self.path in ['/mcp','/sse','/redirect']
        if self.path == '/redirect':
            self.reply(307, None, Location='/mcp'); return
        assert self.headers.get('Authorization') == 'Bearer local-test-token'
        method = data['method']
        seen.append((method, data.get('params')))
        if method == 'initialize':
            self.reply(200, {'jsonrpc':'2.0','id':data['id'],'result':{
                'protocolVersion':'2025-11-25','capabilities':{'tools':{}},
                'serverInfo':{'name':'fixture','version':'1'},'instructions':'Use echo to repeat a message.'}},
                **{'Mcp-Session-Id':'local-test-session'})
            return
        assert self.headers.get('Mcp-Session-Id') == 'local-test-session'
        assert self.headers.get('MCP-Protocol-Version') == '2025-11-25'
        if method == 'notifications/initialized': self.reply(202); return
        if method == 'tools/list':
            # Deliberately paginated; no new tools appear in an already imported package.
            result = {'tools':[tool], 'nextCursor':'second'} if not data['params'].get('cursor') else {'tools':[dict(tool,name='not-imported')]}
        elif method == 'tools/call':
            assert data['params']['name'] == 'echo'
            msg = data['params']['arguments']['message']
            result = {'content':[{'type':'text','text':msg}], 'isError':msg == 'fail'}
        else: self.reply(400); return
        reply = {'jsonrpc':'2.0','id':data['id'],'result':result}
        if self.path == '/sse':
            body = ('id: empty\ndata: \n\n: keepalive\n\nevent: message\ndata: ' + json.dumps(reply) + '\n\n').encode()
            self.send_response(200); self.send_header('Content-Type','text/event-stream')
            self.send_header('Content-Length',str(len(body))); self.end_headers(); self.wfile.write(body)
        else: self.reply(200,reply)

server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
thread = threading.Thread(target=server.serve_forever,daemon=True);thread.start()
endpoint = f'http://127.0.0.1:{server.server_port}'
env = dict(os.environ,RHYVEN_TOKEN_CONNECTOR_TEST='local-test-token')
with tempfile.TemporaryDirectory(prefix='rhyven-connectors-') as d:
    root=Path(d)
    def run(*args, ok=True, environment=env):
        result=subprocess.run([binary,'--workspace',str(root/'home'),*map(str,args)],capture_output=True,text=True,env=environment)
        assert (result.returncode == 0) == ok, (args,result.stdout,result.stderr)
        return json.loads(result.stdout if ok else result.stderr)
    for suffix in ['mcp','sse']:
        app=root/f'{suffix}.json'
        result=run('app','import-mcp','--name',f'test/{suffix}','--endpoint',endpoint+'/'+suffix,
                   '--auth-env','RHYVEN_TOKEN_CONNECTOR_TEST','--include','echo','--out',app)
        assert result['installed_upstream'] is False and result['actions']==1
        assert 'local-test-token' not in app.read_text()
        run('install',app,'--accept-permissions')
        client=Client(root/'home',env=env)
        try:
            desc=client.tool('rhyven_describe',{'category':f'test/{suffix}'})
            assert [f['name'] for f in desc['functions']]==['action_echo']
            args={'category':f'test/{suffix}','function':'action_echo','args':{'message':'hello'}}
            assert client.tool('rhyven_call',args)['content'][0]['text']=='hello'
            args['args']['message']='fail'
            client.tool('rhyven_call',args,error='connector_app')
            before=len(seen)
            args['function']='action_not-imported'
            client.tool('rhyven_call',args,error='not_found')
            assert len(seen)==before
        finally: client.close()
    run('app','import-mcp','--name','test/redirect','--endpoint',endpoint+'/redirect','--include','echo','--out',root/'redirect.json',ok=False)
    assert not (root/'redirect.json').exists()
    before=len(seen)
    run('app','import-mcp','--name','test/missing-auth','--endpoint',endpoint+'/mcp',
        '--auth-env','RHYVEN_TOKEN_CONNECTOR_TEST','--include','echo','--out',root/'noauth.json',environment={k:v for k,v in env.items() if k!='RHYVEN_TOKEN_CONNECTOR_TEST'},ok=False)
    assert len(seen)==before
    spec={'openapi':'3.0.3','paths':{'/items/{id}':{'get':{'operationId':'get_item','parameters':[
        {'in':'path','name':'id','required':True,'schema':{'type':'string'}},
        {'in':'query','name':'q','schema':{'type':'string'}}]}},'/items':{'post':{'operationId':'create_item',
        'requestBody':{'required':True,'content':{'application/json':{'schema':{'type':'object','properties':{'name':{'type':'string'}},'required':['name']}}}}}}}}
    source=root/'openapi.json';source.write_text(json.dumps(spec));app=root/'http.json'
    run('app','import-openapi',source,'--name','test/http','--endpoint',endpoint+'/api',
        '--include','get_item','--include','create_item','--out',app)
    run('app','package',app,'--out',root/'http.rhyven.json')
    run('install',app,'--accept-permissions')
    client=Client(root/'home',env=env)
    try:
        result=client.tool('rhyven_call',{'category':'test/http','function':'action_get_item','args':{'id':'hello world','q':'x&y'}})
        assert result['status']==200 and result['body']['path']=='/api/items/hello%20world?q=x%26y',result
        result=client.tool('rhyven_call',{'category':'test/http','function':'action_create_item','args':{'body':{'name':'test'}}})
        assert result=={'status':200,'body':{'created':{'name':'test'}}}
    finally: client.close()
    # Real shared REST dispatch uses the same connector path as local MCP.
    import socket,time
    with socket.socket() as sock: sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    token='connector-shared-test-token-12345'
    shared=subprocess.Popen([binary,'--workspace',str(root/'home'),'serve','--port',str(port)],env=dict(env,RHYVEN_SERVE_TOKEN=token),stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
    try:
        for _ in range(100):
            try:
                with socket.create_connection(('127.0.0.1',port),timeout=.1):break
            except OSError:time.sleep(.05)
        request=Request(f'http://127.0.0.1:{port}/categories/test/mcp/functions/action_echo',data=b'{"message":"REST parity"}',headers={'Content-Type':'application/json','Authorization':'Bearer '+token})
        with urlopen(request,timeout=10) as response:assert json.load(response)['content'][0]['text']=='REST parity'
    finally:shared.terminate();shared.wait(timeout=5)
    # An unsupported constraint fails rather than broadening validation or writing a package.
    spec['paths']['/items/{id}']['get']['parameters'][0]['schema']['pattern']='^[a-z]+$'
    source.write_text(json.dumps(spec))
    run('app','import-openapi',source,'--name','test/unsupported','--endpoint',endpoint+'/api','--include','get_item','--out',root/'unsupported.json',ok=False)
    assert not (root/'unsupported.json').exists()
server.shutdown();server.server_close()
print('PASS: MCP JSON/SSE, pagination, auth/session lifecycle, tool allowlist, tool errors, no redirects, OpenAPI path/query/body mapping, package/install, MCP/REST parity, strict import rejection')
