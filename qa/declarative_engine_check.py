"""Exercise declarative arithmetic/guards and typed queries through MCP and REST."""
import json, os, socket, subprocess, sys, tempfile, time, urllib.request
from pathlib import Path
from universal_market_check import Client
binary=str(Path(sys.argv[1]).resolve())
def obj(props,required):return dict(type='object',properties=props,required=required,additionalProperties=False)
with tempfile.TemporaryDirectory(prefix='rhyven-engine-') as temp:
    home=Path(temp)
    def cli(*args):return json.loads(subprocess.check_output([binary,'--home',temp,*args]))
    package={'format':2,'name':'test/engine','version':'0.1.0','publisher':'test','description':'Engine transport acceptance','guide':'Test fixture','hosting':{'mode':'local'},'permissions':['state.read','state.write'],'objects':{'stock':{'schema':obj({'quantity':{'type':'integer','default':10,'minimum':0},'label':{'type':'string'}},['quantity','label']),'protected_fields':['quantity']}},'actions':{'withdraw':{'description':'Subtract stock atomically','operation':'update','object':'stock','input':obj({'id':{'type':'string'},'expected_revision':{'type':'integer','minimum':1},'amount':{'type':'integer','minimum':1}},['id','expected_revision','amount']),'condition':{'op':'ge','args':[{'field':'quantity'},{'arg':'amount'}]},'expressions':{'quantity':{'op':'sub','args':[{'field':'quantity'},{'arg':'amount'}]}}}},'tests':[]}
    file=home/'app.json';file.write_text(json.dumps(package));cli('app','validate',str(file));cli('install',str(file),'--accept-permissions')
    client=Client(home/'collections/global')
    def mcp(function,args):
        response=client.request('tools/call',{'name':'rhyven_call','arguments':{'category':'test/engine','function':function,'args':args}})
        assert not response.get('isError'),response
        return json.loads(response['content'][0]['text'])
    try:
        row=mcp('object_stock_create',{'data':{'label':'bolts'}})
        args={'id':row['id'],'expected_revision':1,'amount':3,'request_id':'cross-adapter'}
        result=mcp('action_withdraw',args);assert result['data']['quantity']==7
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        token='declarative-test-token-0123456789'
        server=subprocess.Popen([binary,'--home',temp,'serve','--port',str(port)],env=dict(os.environ,RHYVEN_SERVE_TOKEN=token),stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):break
                except OSError:
                    assert server.poll() is None,server.stderr.read()
                    time.sleep(.05)
            def http(function,args):
                req=urllib.request.Request(f'http://127.0.0.1:{port}/categories/test/engine/functions/{function}',data=json.dumps(args).encode(),headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
                with urllib.request.urlopen(req) as response:return json.load(response)
            updated=http('action_withdraw',{'id':row['id'],'expected_revision':2,'amount':2});assert updated['data']['quantity']==5
            query={'where':{'quantity':{'ge':3,'lt':8},'label':{'in':['bolts']}},'order_by':[{'field':'quantity','direction':'desc'}]}
            assert http('object_stock_query',query)==mcp('object_stock_query',query)
            assert mcp('object_stock_query',query)['total']==1
        finally:server.terminate();server.wait(timeout=5)
    finally:client.close()
print('PASS: package validation, installation, MCP and REST arithmetic, current-field conditions and typed query/sort parity')
