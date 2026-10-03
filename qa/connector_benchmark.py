"""Reproducible visible-input benchmark against an ALREADY RUNNING reference MCP.
Usage: python3 qa/connector_benchmark.py BINARY http://127.0.0.1:38721/mcp OUT_DIR
Requires tiktoken==0.12.0. Does not install/start upstream, invoke a model, or claim billed usage.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.request
import tiktoken
from universal_market_check import Client

binary=str(Path(sys.argv[1]).resolve());endpoint=sys.argv[2];out=Path(sys.argv[3])
out.mkdir(parents=True,exist_ok=True)
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
session=None;seq=0

def upstream(method,params=None,notification=False):
    global seq,session
    seq+=1
    body={'jsonrpc':'2.0','method':method}
    if not notification:body['id']=seq
    if params is not None:body['params']=params
    headers={'Content-Type':'application/json','Accept':'application/json, text/event-stream','MCP-Protocol-Version':'2025-11-25'}
    if session:headers['Mcp-Session-Id']=session
    with opener.open(urllib.request.Request(endpoint,data=json.dumps(body).encode(),headers=headers),timeout=20) as response:
        session=response.headers.get('Mcp-Session-Id',session)
        if notification:return None
        if response.headers.get('Content-Type','').startswith('text/event-stream'):
            data='';total=0
            while True:
                line=response.readline();total+=len(line)
                assert total<=1048576,'response too large'
                if not line or not line.strip():
                    if data.strip():
                        value=json.loads(data)
                        if value.get('id')==seq:break
                    data=''
                    assert line,'stream ended'
                elif line.startswith(b'data:'):data+=line[5:].decode().lstrip(' ')
        else:value=json.load(response)
    assert 'error' not in value,value
    return value['result']

init=upstream('initialize',{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'rhyven-token-benchmark','version':'1'}})
upstream('notifications/initialized',notification=True)
tools=[];cursor=None
while True:
    page=upstream('tools/list',{'cursor':cursor} if cursor else {})
    tools.extend(page['tools']);cursor=page.get('nextCursor')
    if not cursor:break
selected=[t for t in tools if t['name'] in ('echo','get-sum')]
assert len(selected)==2
call_args={'a':17,'b':25}
result=upstream('tools/call',{'name':'get-sum','arguments':call_args})
assert not result.get('isError')
if session:
    opener.open(urllib.request.Request(endpoint,method='DELETE',headers={'Mcp-Session-Id':session,'MCP-Protocol-Version':'2025-11-25'}),timeout=5).close()

with tempfile.TemporaryDirectory(prefix='rhyven-connector-benchmark-') as temp:
    root=Path(temp);app=root/'app.json'
    def cli(*args):return json.loads(subprocess.check_output([binary,'--workspace',str(root/'home'),*map(str,args)]))
    imported=cli('app','import-mcp','--name','test/everything','--endpoint',endpoint,'--include','echo','--include','get-sum','--out',app)
    cli('install',app,'--accept-permissions')
    c=Client(root/'home',elicitation=False)
    try:
        # Existing helper initializes automatically; a separate probe captures the exact init guidance.
        p=subprocess.Popen([binary,'--workspace',str(root/'home'),'mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        p.stdin.write(json.dumps({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'probe','version':'1'}}})+'\n');p.stdin.flush()
        rhyven_init=json.loads(p.stdout.readline())['result'];p.stdin.close();p.wait(timeout=5)
        definitions=c.request('tools/list',{})['tools']
        categories=c.tool('rhyven_categories',{})
        description=c.tool('rhyven_describe',{'category':'test/everything'})
        wrapped=c.tool('rhyven_call',{'category':'test/everything','function':'action_get-sum','args':call_args})
        assert wrapped==result,(wrapped,result)
    finally:c.close()
    def canonical(v):
        # Fixed placeholders remove irrelevant ephemeral port/workspace variation.
        return json.dumps(v,ensure_ascii=False,sort_keys=True,separators=(',',':')).replace(str(root/'home'),'<workspace>').replace(endpoint,'http://127.0.0.1:3001/mcp')
    def exchange(name,args,response):
        return [{'role':'assistant','tool_call':{'name':name,'arguments':args}}, {'role':'tool','name':name,'content':response}]
    task={'role':'user','content':'Use the Everything server to calculate 17 + 25, then report the result.'}
    direct_messages=[{'role':'system','content':init.get('instructions','')},task]
    direct_calls=exchange('get-sum',call_args,result)
    wrapper_messages=[{'role':'system','content':rhyven_init.get('instructions','')},task]
    discovery=exchange('rhyven_categories',{},categories)
    describe=exchange('rhyven_describe',{'category':'test/everything'},description)
    wrapped_call=exchange('rhyven_call',{'category':'test/everything','function':'action_get-sum','args':call_args},wrapped)
    traces={
        'direct_all_tools':[{'tools':tools,'messages':direct_messages},{'tools':tools,'messages':direct_messages+direct_calls}],
        'direct_same_two_tools':[{'tools':selected,'messages':direct_messages},{'tools':selected,'messages':direct_messages+direct_calls}],
        'rhyven_cold_discovery':[{'tools':definitions,'messages':wrapper_messages},
            {'tools':definitions,'messages':wrapper_messages+discovery},
            {'tools':definitions,'messages':wrapper_messages+discovery+describe},
            {'tools':definitions,'messages':wrapper_messages+discovery+describe+wrapped_call}],
        'rhyven_known_category':[{'tools':definitions,'messages':wrapper_messages},
            {'tools':definitions,'messages':wrapper_messages+describe},
            {'tools':definitions,'messages':wrapper_messages+describe+wrapped_call}],
    }
    # Repeated workflows retain discovery context, as a normal un-compacted session does.
    for label,defs,msg in [('direct_all_tools',tools,direct_messages+direct_calls),('direct_same_two_tools',selected,direct_messages+direct_calls),('rhyven',definitions,wrapper_messages+discovery+describe+wrapped_call)]:
        traces[label+'_next_task']=[{'tools':defs,'messages':msg+[task]},{'tools':defs,'messages':msg+[task]+(wrapped_call if label=='rhyven' else direct_calls)}]
    artifact={'upstream_initialize':init,'upstream_tools':tools,'rhyven_initialize':rhyven_init,'categories':categories,'description':description,'result':result,'import':imported,'traces':traces}
    text=canonical(artifact).replace(str(root),'<temporary-directory>')
    (out/'trace.json').write_text(text+'\n')
    report={'method':'Exact tokenizer counts of canonical visible JSON input fixtures, including guides and cumulative conversation history. Synthetic fixed tool-call schedule; no LLM was invoked. Not provider-billed input usage or a reasoning-quality benchmark.',
            'reference_server':'@modelcontextprotocol/server-everything@2026.8.31',
            'reference_source':'https://github.com/modelcontextprotocol/servers/tree/main/src/everything',
            'upstream_server_info':init['serverInfo'],'upstream_tool_count':len(tools),'selected_tools':[t['name'] for t in selected],
            'task':task['content'],'result_matches':True,'tokenizer_version':tiktoken.__version__,
            'trace_sha256':hashlib.sha256((text+'\n').encode()).hexdigest(),'encodings':{}}
    for encoding in ['o200k_base','cl100k_base']:
        tokenizer=tiktoken.get_encoding(encoding)
        count=lambda v:len(tokenizer.encode(canonical(v),disallowed_special=()))
        report['encodings'][encoding]={
            'components':{'direct_all_tools':count(tools),'direct_selected_tools':count(selected),'upstream_guide':count(init.get('instructions','')),
                'rhyven_three_tools':count(definitions),'rhyven_guide':count(rhyven_init.get('instructions','')),
                'categories_response':count(categories),'describe_response_including_guide_and_contract':count(description)},
            'scenarios':{name:{'input_tokens_by_turn':[count(turn) for turn in turns],'cumulative_input_tokens':sum(count(turn) for turn in turns)} for name,turns in traces.items()}}
    (out/'results.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
