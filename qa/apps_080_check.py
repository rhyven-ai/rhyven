"""Exercise 0.8 apps through the three-tool MCP interface in isolated collections.
Usage: python3 qa/apps_080_check.py BINARY [change-verifier|onboarding-monitor IMAGE_ID_FILE]
No model calls, external notifications or publication occur.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from universal_market_check import Client

binary = str(Path(sys.argv[1]).resolve())
app = sys.argv[2] if len(sys.argv) > 2 else 'design-review'
image = Path(sys.argv[3]).read_text().strip() if len(sys.argv) > 3 else None

with tempfile.TemporaryDirectory(prefix='rhyven-080-app-') as directory:
    home = Path(directory)
    def cli(*args):
        p = subprocess.run([binary, '--home', directory, '--collection', 'trial', *args], capture_output=True, text=True, timeout=180)
        assert p.returncode == 0, (args, p.stdout, p.stderr)
        return json.loads(p.stdout)
    path = home/'app.json'
    command = ['app','package','apps/'+app,'--out',str(path)]
    if image:
        command += ['--image',image]
    cli(*command)
    cli('install',str(path),'--accept-permissions')
    cli('install','rhyven/work-management','--accept-permissions')
    cli('install','rhyven/project-knowledge','--accept-permissions')
    daemon = False
    clients = []
    try:
        if image:
            cli('daemon','start')
            daemon = True
        lead = Client(home/'collections/trial',actor='lead',response_timeout=45)
        other = Client(home/'collections/trial',actor='reviewer',response_timeout=45)
        clients += [lead,other]
        category = 'rhyven/'+app
        assert any(a['name']==category for a in lead.tool('rhyven_categories',{})['apps'])
        assert lead.tool('rhyven_describe',{'category':category})['functions']
        if app=='design-review':
            review=lead.call(category,'action_start',{'question':'How should we verify a fix?','perspectives':['regression testing']})['review']
            other.call(category,'action_submit',dict(review_id=review['id'],task_id='1',approach='Run identical tests before and after',assumptions='Python snapshots',tradeoffs='Only covers supplied cases',checks='Check setup errors separately',model='deterministic-test-fixture'))
            result=lead.call(category,'action_decide',dict(review_id=review['id'],selected_tasks=['1'],rationale='Use before/after assertion evidence'))
            assert other.call(category,'action_get',{'review_id':review['id']})==result
        elif app=='change-verifier':
            def files(code):return [{'path':'calc.py','content':code},{'path':'test_existing.py','content':'from calc import add\ndef test_existing(): assert add(0,0)==0\n'}]
            run=lead.call(category,'action_prepare',dict(baseline=files('def add(a,b): return a-b\n'),candidate=files('def add(a,b): return a+b\n'),regression=[{'path':'test_fix.py','content':'from calc import add\ndef test_fix(): assert add(1,2)==3\n'}]))
            lead.call(category,'action_run',{'run_id':run['run_id']})
            deadline=time.monotonic()+45
            while time.monotonic()<deadline:
                result=other.call(category,'action_get',{'run_id':run['run_id']})
                if result['status']!='running':break
                time.sleep(.2)
            assert result['report']['outcome']=='supplied_case_verified',result
            cli('service','restart',category)
            assert other.call(category,'action_get',{'run_id':run['run_id']})==result
        else:
            customer=lead.call(category,'action_open',dict(customer_id='demo',requirements=['contact'],deadline=int(time.time())+300))['customer']
            customer=lead.call(category,'action_ingest',dict(customer_id='demo',event_id='document-1',kind='document',source='fixture:contact',requirement='contact'))['customer']
            items=other.call(category,'action_claim',{})['items']
            assert len(items)==2
            customer=other.call(category,'action_review',dict(customer_id='demo',expected_revision=customer['revision'],requirement='contact',accepted=True,note='Fixture contact verified'))['customer']
            result=other.call(category,'action_handoff',dict(customer_id='demo',expected_revision=customer['revision']))
            for item in items:other.call(category,'action_acknowledge',dict(event_id=item['event']['id'],lease_token=item['lease_token']))
            cli('service','restart',category)
            assert lead.call(category,'action_get',{'customer_id':'demo'})==result
        task=lead.call('rhyven/work-management','object_task_create',{'data':{'title':'Review '+app+' result'}})
        note=lead.call('rhyven/project-knowledge','action_remember',{'title':app+' evidence','body':json.dumps(result),'topic':'acceptance'})
        assert other.call('rhyven/work-management','object_task_get',{'id':task['id']})['data']['title']=='Review '+app+' result'
        assert other.call('rhyven/project-knowledge','object_note_get',{'id':note['id']})['data']['body']==json.dumps(result)
        print(json.dumps({'app':app,'mcp_roundtrip':True,'shared_results':True,'service_restart':bool(image)}))
    finally:
        for client in clients:client.close()
        if daemon:cli('daemon','stop')
