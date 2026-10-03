"""Compare filtered direct MCP, old Rhyven and new Rhyven over resumed Codex sessions.
Requires Codex login; reference servers and SDK versions match multi_mcp_benchmark.py.
Never publishes or changes user configuration. Session traces contain only synthetic fixtures.
"""
import argparse,hashlib,json,os,subprocess,time,urllib.request
from pathlib import Path
parser=argparse.ArgumentParser(description="Live Codex benchmark; uses existing login and writes only local fixtures/results. Reference MCP dependencies must already be installed.")
parser.add_argument('binary',type=Path)
parser.add_argument('baseline',type=Path)
parser.add_argument('modules',type=Path)
parser.add_argument('out',type=Path)
parser.add_argument('--repeats',type=int,default=2)
parser.add_argument('--model',default='gpt-6-astra')
args=parser.parse_args()
if args.repeats < 1:parser.error("--repeats must be positive")
repo=Path(__file__).resolve().parents[1]
out=args.out.resolve();out.mkdir(parents=True,exist_ok=True)
binary=str(args.binary.resolve());baseline=str(args.baseline.resolve())
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
fixture=out/'fixture';fixture.mkdir(exist_ok=True)
(fixture/'plan.json').write_text('{"project":"alpha","base":17}\n')
home=out/'home';empty=out/'empty';empty.mkdir(exist_ok=True)
selections={'filesystem':['read_text_file','list_directory'],'memory':['search_nodes','open_nodes'],'everything':['echo','get-sum']}
results=[]
versions={name:json.loads((args.modules.resolve()/f'@modelcontextprotocol/server-{name}/package.json').read_text())['version'] for name in selections}
assert all(version=='2026.8.31' for version in versions.values()), versions
metadata={'codex_version':subprocess.check_output(['codex','--version'],text=True).strip(),'model':args.model,'reasoning_effort':'medium','reference_versions':versions,'before_sha256':hashlib.sha256(Path(baseline).read_bytes()).hexdigest(),'after_sha256':hashlib.sha256(Path(binary).read_bytes()).hexdigest(),'repeats':args.repeats,'usage_semantics':'Codex resume reports cumulative session usage; subtract preceding turn totals to derive per-task usage.'}
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
gateway=subprocess.Popen(['node',str(repo/'qa/fixtures/mcp-benchmark-gateway.mjs'),str(args.modules.resolve()),str(fixture)],stdout=subprocess.PIPE,stderr=(out/'gateway.stderr').open('w'),text=True)
try:
    endpoint=json.loads(gateway.stdout.readline())['endpoint']
    memory=Upstream(endpoint+'/memory')
    memory.call('create_entities',{'entities':[{'name':'alpha','entityType':'project','observations':['offset=25']}]});memory.close()
    for name,selected in selections.items():
        package=out/(name+'.json')
        import_args=[binary,'--workspace',str(home),'app','import-mcp','--name','test/'+name,'--endpoint',endpoint+'/'+name,'--out',str(package)]
        for tool in selected:import_args+=['--include',tool]
        subprocess.run(import_args,check=True,stdout=subprocess.DEVNULL)
        subprocess.run([binary,'--workspace',str(home),'install',str(package),'--accept-permissions'],check=True,stdout=subprocess.DEVNULL)
    for repeat in range(1,args.repeats+1):
        arms=['direct_filtered','before','after']
        if repeat%2==0:arms.reverse()
        for arm in arms:
            thread=None
            for turn,base in enumerate([17,31,46],1):
                (fixture/'plan.json').write_text(json.dumps({'project':'alpha','base':base})+'\n')
                cmd=['codex','exec','--ignore-user-config','--json','--sandbox','read-only','--skip-git-repo-check','-C',str(empty),'-m',args.model,'-c','model_reasoning_effort="medium"']
                if arm!='direct_filtered':
                    executable=binary if arm=='after' else baseline
                    config='mcp_servers.rhyven={command='+json.dumps(executable)+',args='+json.dumps(['--workspace',str(home),'mcp'])+',required=true}'
                    cmd+=['-c',config]
                    for tool in ['rhyven_categories','rhyven_describe','rhyven_call']:
                        cmd+=['-c','mcp_servers.rhyven.tools.'+tool+'.approval_mode="approve"']
                else:
                    for name,selected in selections.items():
                        config='mcp_servers.'+name+'={url='+json.dumps(endpoint+'/'+name)+',required=true,enabled_tools='+json.dumps(selected)+'}'
                        cmd+=['-c',config]
                prompt=f'Use only MCP tools for this task. Do not use shell, filesystem tools outside MCP, external network, or delegate. Read {fixture}/plan.json through the Filesystem capability. Find that project in the Memory capability and retrieve its offset observation. Use the Everything capability to add the base to that offset. Discover the available tools as needed. Return the project, base, offset, and computed sum. Do not change any data.'
                if turn>1:prompt='The plan file has changed. Read it again and recompute from current data. '+prompt
                if thread:cmd+=['resume',thread]
                label=f'{repeat}-{arm}-{turn}';start=time.monotonic()
                print('START '+label,flush=True)
                with (out/(label+'.jsonl')).open('w') as stdout,(out/(label+'.stderr')).open('w') as stderr:
                    proc=subprocess.run(cmd+[prompt],stdout=stdout,stderr=stderr,stdin=subprocess.DEVNULL,timeout=240)
                events=[json.loads(line) for line in (out/(label+'.jsonl')).read_text().splitlines() if line.startswith('{')]
                threads=[e['thread_id'] for e in events if e['type']=='thread.started']
                if threads:
                    if thread:assert threads[-1]==thread, 'Resume changed session'
                    thread=threads[-1]
                usage=[e['usage'] for e in events if e['type']=='turn.completed']
                items=[e['item'] for e in events if e['type']=='item.completed']
                calls=[i for i in items if i['type']=='mcp_tool_call']
                arithmetic=[i for i in calls if i['tool']=='get-sum' or (i['tool']=='rhyven_call' and i['arguments'].get('function')=='action_get-sum')]
                assert len(arithmetic)==1
                sum_args=arithmetic[0]['arguments']
                if arithmetic[0]['tool']=='rhyven_call':sum_args=sum_args['args']
                assert sum_args=={'a':base,'b':25} or sum_args=={'a':25,'b':base}
                final=[i['text'] for i in items if i['type']=='agent_message']
                success=proc.returncode==0 and bool(usage and calls and final) and all(i['status']=='completed' and not i.get('error') for i in calls) and not any(i['type']=='command_execution' for i in items) and all(str(v) in final[-1] for v in ['alpha',base,25,base+25])
                row={'run':label,'arm':arm,'repeat':repeat,'turn':turn,'thread':thread,'exit_code':proc.returncode,'success':success,'seconds':round(time.monotonic()-start,2),'usage':usage,'items':items}
                results.append(row);(out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
                print(json.dumps({k:row[k] for k in ['run','success','usage','seconds']}),flush=True)
                if not success:raise RuntimeError('Invalid run; inspect '+label+' artifacts')
finally:
    gateway.terminate()
    try:gateway.wait(timeout=15)
    except subprocess.TimeoutExpired:gateway.kill();gateway.wait()
