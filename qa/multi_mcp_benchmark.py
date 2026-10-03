"""Multiple real MCP servers; captured results plus modeled visible-input token counts.
python3 qa/multi_mcp_benchmark.py BINARY NODE_MODULES OUT_DIR
Requires separately installed official reference servers at 2026.8.31 and tiktoken 0.12.0.
Does not invoke an LLM, install dependencies, or publish anything.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.request
import tiktoken
from universal_market_check import Client

binary = str(Path(sys.argv[1]).resolve())
modules = Path(sys.argv[2]).resolve()
out = Path(sys.argv[3]).resolve(); out.mkdir(parents=True, exist_ok=True)
repo = Path(__file__).resolve().parents[1]
servers = ['filesystem', 'memory', 'everything']
versions = {}
for name in servers:
    package = json.loads((modules / f'@modelcontextprotocol/server-{name}/package.json').read_text())
    assert package['version'] == '2026.8.31', package['version']
    versions[name] = package['version']

class Upstream:
    def __init__(self, endpoint):
        self.endpoint=endpoint;self.seq=0;self.session=None
        self.opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.init=self.request('initialize',{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'multi-mcp-test','version':'1'}})
        self.request('notifications/initialized',notification=True)
        self.tools=[];cursor=None
        while True:
            result=self.request('tools/list',{'cursor':cursor} if cursor else {})
            self.tools.extend(result['tools']);cursor=result.get('nextCursor')
            if not cursor:break
    def request(self, method, params=None, notification=False):
        self.seq+=1;body={'jsonrpc':'2.0','method':method}
        if not notification:body['id']=self.seq
        if params is not None:body['params']=params
        headers={'Content-Type':'application/json','Accept':'application/json, text/event-stream','MCP-Protocol-Version':'2025-11-25'}
        if self.session:headers['Mcp-Session-Id']=self.session
        with self.opener.open(urllib.request.Request(self.endpoint,data=json.dumps(body).encode(),headers=headers),timeout=30) as r:
            self.session=r.headers.get('Mcp-Session-Id',self.session)
            if notification:return
            reply=json.load(r)
        assert 'error' not in reply,reply
        return reply['result']
    def call(self,name,args):
        r=self.request('tools/call',{'name':name,'arguments':args});assert not r.get('isError'),r;return r
    def close(self):
        if self.session:
            self.opener.open(urllib.request.Request(self.endpoint,method='DELETE',headers={'Mcp-Session-Id':self.session,'MCP-Protocol-Version':'2025-11-25'}),timeout=10).close()
            self.session=None

def exchange(name,args,result):
    return [{'role':'assistant','tool_call':{'name':name,'arguments':args}},
            {'role':'tool','name':name,'content':result}]

def frames(definitions, guide, task, exchanges, history=None):
    messages=history or [{'role':'system','content':guide}]
    messages=messages+[{'role':'user','content':task}]
    turns=[{'tools':definitions,'messages':messages.copy()}]
    for step in exchanges:
        messages=messages+step
        turns.append({'tools':definitions,'messages':messages.copy()})
    return turns,messages

with tempfile.TemporaryDirectory(prefix='rhyven-multiple-mcps-') as temp:
    root=Path(temp);fixture=root/'fixture';fixture.mkdir()
    (fixture/'plan.json').write_text('{"project":"alpha","base":17}\n')
    home=root/'home'
    gateway=subprocess.Popen(['node',str(repo/'qa/fixtures/mcp-benchmark-gateway.mjs'),str(modules),str(fixture)],
                             stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    clients={};rhyven=None
    try:
        endpoint=json.loads(gateway.stdout.readline())['endpoint']
        clients={name:Upstream(endpoint+'/'+name) for name in servers}
        clients['memory'].call('create_entities',{'entities':[{'name':'alpha','entityType':'project','observations':['offset=25']}]})
        selections={'filesystem':['read_text_file','list_directory'],'memory':['search_nodes','open_nodes'],'everything':['echo','get-sum']}
        def cli(*args):
            return json.loads(subprocess.check_output([binary,'--workspace',str(home),*map(str,args)]))
        for name in servers:
            args=['app','import-mcp','--name','test/'+name,'--endpoint',endpoint+'/'+name,'--out',root/(name+'.json')]
            for tool in selections[name]:args+=['--include',tool]
            cli(*args);cli('install',root/(name+'.json'),'--accept-permissions')
        rhyven=Client(home,elicitation=False)
        probe=subprocess.Popen([binary,'--workspace',str(home),'mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        probe.stdin.write(json.dumps({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'probe','version':'1'}}})+'\n');probe.stdin.flush()
        rhyven_init=json.loads(probe.stdout.readline())['result'];probe.stdin.close();probe.wait(timeout=5)
        defs=rhyven.request('tools/list',{})['tools'];categories=rhyven.tool('rhyven_categories',{})
        direct_results=[];wrapped_results=[];steps=[];descriptions={}
        calls=[('filesystem','read_text_file',{'path':str(fixture/'plan.json')})]
        for index in range(3):
            server,tool,args=calls[index]
            description=rhyven.tool('rhyven_describe',{'category':'test/'+server,'function':'action_'+tool})
            descriptions[server]=description
            direct=clients[server].call(tool,args)
            wrapped=rhyven.tool('rhyven_call',{'category':'test/'+server,'function':'action_'+tool,'args':args})
            assert direct==wrapped,(server,direct,wrapped)
            direct_results.append(direct);wrapped_results.append(wrapped)
            if index==0:
                plan=json.loads(direct['content'][0]['text'])
                assert plan=={'project':'alpha','base':17}
                calls.append(('memory','search_nodes',{'query':plan['project']}))
            elif index==1:
                graph=json.loads(direct['content'][0]['text'])
                offset=int(graph['entities'][0]['observations'][0].split('=')[1])
                calls.append(('everything','get-sum',{'a':plan['base'],'b':offset}))
        assert '42' in direct_results[-1]['content'][0]['text']
        task_single=f'Read {fixture}/plan.json using Filesystem and report its project and base.'
        task_cross=f'Read {fixture}/plan.json, find that project in Memory, then use Everything to add its base to the stored offset. Report the sum.'
        def definitions(names, selection=None):
            result=[]
            for name in names:
                for tool in clients[name].tools:
                    if selection is not None and tool['name'] not in selection.get(name,[]):continue
                    # Namespacing is required when multiple MCP servers expose overlapping tool names.
                    result.append(dict(tool,name=name+'__'+tool['name']))
            return result
        def guides(names):
            return '\n\n'.join(f'{name}:\n{clients[name].init.get("instructions","")}' for name in names)
        traces={};histories={}
        for workload,count,task in [('single_active_server',1,task_single),('cross_server_workflow',3,task_cross)]:
            required={name:[tool] for name,tool,_ in calls[:count]}
            direct_steps=[exchange(name+'__'+tool,args,result) for (name,tool,args),result in zip(calls[:count],direct_results)]
            configs=[('direct_all',definitions(servers),guides(servers)),
                     ('direct_same_six_tools',definitions(servers,selections),guides(servers)),
                     ('direct_task_filtered',definitions(list(required),required),guides(list(required)))]
            for label,tool_defs,guide in configs:
                key=workload+'/'+label
                traces[key],histories[key]=frames(tool_defs,guide,task,direct_steps)
                traces[key+'_repeat'],_=frames(tool_defs,guide,task,direct_steps,histories[key])
            wrapper_steps=[]
            for (name,tool,args),result in zip(calls[:count],wrapped_results):
                wrapper_steps.append(exchange('rhyven_describe',{'category':'test/'+name,'function':'action_'+tool},descriptions[name]))
                wrapper_steps.append(exchange('rhyven_call',{'category':'test/'+name,'function':'action_'+tool,'args':args},result))
            discovery=exchange('rhyven_categories',{},categories)
            for label,prefix in [('rhyven_cold',[discovery]),('rhyven_known_categories',[])]:
                key=workload+'/'+label
                traces[key],histories[key]=frames(defs,rhyven_init.get('instructions',''),task,prefix+wrapper_steps)
                calls_only=[wrapper_steps[i] for i in range(1,len(wrapper_steps),2)]
                traces[key+'_repeat'],_=frames(defs,rhyven_init.get('instructions',''),task,calls_only,histories[key])
        def canonical(v):
            return json.dumps(v,ensure_ascii=False,sort_keys=True,separators=(',',':')).replace(str(root),'<test-root>').replace(endpoint,'http://127.0.0.1:3001')
        artifact={'servers':{k:{'initialize':v.init,'tools':v.tools} for k,v in clients.items()},'categories':categories,
                  'descriptions':descriptions,'direct_results':direct_results,'wrapped_results':wrapped_results,'traces':traces}
        capture=canonical(artifact)+'\n';(out/'trace.json').write_text(capture)
        report={'method':'Real calls to three distinct official MCP servers, equal results through Rhyven. Exact tokenizer counts for fixed synthetic model-input schedules, including guides and retained history. No model invoked; not billed usage or reasoning/latency measurement.',
                'reference_package_versions':versions,'sdk_version':json.loads((modules/'@modelcontextprotocol/sdk/package.json').read_text())['version'],'upstream_tool_counts':{k:len(v.tools) for k,v in clients.items()},'wrapped_tools':selections,
                'actual_call_order':[{'server':n,'tool':t} for n,t,a in calls],'results_match':True,'final_sum':42,
                'transport':'Local test-only HTTP-to-stdio gateway using the official SDK; tool schemas, server instructions and results forwarded unchanged.',
                'tokenizer_version':tiktoken.__version__,'capture_sha256':hashlib.sha256(capture.encode()).hexdigest(),
                'assumptions':['Sequential dependent calls; one modeled input per decision and one final answer input.',
                               'Direct all loads every tool from all three servers on each modeled turn.',
                               'Direct same-six matches the six imported tools; task-filtered is an optimistic baseline with only required tools and active-server guides.',
                               'Rhyven selects the required function at describe time; function names are assumed known for both filtered baselines and Rhyven. No discovery search errors modeled.',
                               'Repeats retain all prior tool-call history and descriptions; prior final answer text is excluded equally.',
                               'No cache discounts, compaction, batching, hidden harness prompts, generated reasoning or provider-specific tool serialization.'],
                'encodings':{}}
        for encoding in ['o200k_base','cl100k_base']:
            tokenizer=tiktoken.get_encoding(encoding)
            count=lambda v:len(tokenizer.encode(canonical(v),disallowed_special=()))
            report['encodings'][encoding]={'scenarios':{key:{'input_tokens_by_turn':[count(x) for x in turns],
                'cumulative_input_tokens':sum(count(x) for x in turns)} for key,turns in traces.items()}}
        # Sensitivity check: many hosts omit MCP annotations/title/outputSchema
        # when exposing a function to a model. Strip those from BOTH paths.
        for encoding in ['o200k_base','cl100k_base']:
            tokenizer=tiktoken.get_encoding(encoding)
            def stripped(turn):
                return dict(turn,tools=[{k:tool[k] for k in ['name','description','inputSchema'] if k in tool} for tool in turn['tools']])
            report['encodings'][encoding]['function_schema_only']={key:{
                'cumulative_input_tokens':sum(len(tokenizer.encode(canonical(stripped(x)),disallowed_special=())) for x in turns)
            } for key,turns in traces.items()}
        (out/'results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
    finally:
        if rhyven:rhyven.close()
        for c in clients.values():c.close()
        gateway.terminate()
        try:gateway.wait(timeout=10)
        except subprocess.TimeoutExpired:gateway.kill();gateway.wait()
