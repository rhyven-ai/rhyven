"""Exercise native scaffolding, packaging, consent and execution through the public CLI."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix='rhyven-script-cli-') as temporary:
    root = Path(temporary)
    home = root / 'home'

    def cli(*args, success=True):
        result = subprocess.run([binary, '--home', str(home), *map(str, args)],
                                capture_output=True, text=True, timeout=120,
                                env=dict(os.environ, RHYVEN_TEST_PARENT_SECRET='must-not-inherit'))
        assert (result.returncode == 0) == success, result.stderr
        return json.loads(result.stdout if success else result.stderr)

    for language in ('python', 'javascript'):
        app = 'test/' + language
        source = root / language
        cli('app', 'init', app, '--runtime', language, '--dir', source)
        cli('app', 'validate', source)
        assert cli('app', 'test', source, success=False)['code'] == 'permission_review_required'
        assert cli('app', 'test', source, '--allow-container', success=False)['code'] == 'permission_review_required'
        assert cli('app', 'test', source, '--allow-host')['passed']
        bundle = root / (language + '.json')
        packaged = cli('app', 'package', source, '--out', bundle)
        assert packaged['behavior_tests_run'] is False
        assert isinstance(json.loads(bundle.read_text())['files'], dict)
        assert cli('install', bundle, success=False)['code'] == 'permission_review_required'
        cli('install', bundle, '--accept-permissions')
        manifest = cli('call', 'rhyven_describe', json.dumps({'category': app}))
        assert 'files' not in manifest['contract']
        request = {'category': app, 'function': 'action_analyze', 'args': {
            'text': 'hello $(not-a-command)', 'request_id': 'retry-' + language}}
        first = cli('call', 'rhyven_call', json.dumps(request))
        assert first['words'] == 2 and first['calls'] == 1
        assert cli('call', 'rhyven_call', json.dumps(request)) == first
        print('PASS:', language, 'scaffold, validate, explicit tests, package, install and retry', flush=True)

    # Verify child processes do not inherit secrets or interpreter injection variables.
    package = json.loads((root / 'python.json').read_text())
    package['name'] = 'test/environment'
    package['files']['main.py'] = "import json,os;print(json.dumps({'result':{'words':int('RHYVEN_TEST_PARENT_SECRET' in os.environ),'sha256':'test','calls':1}}))"
    bundle = root / 'environment.json'
    bundle.write_text(json.dumps(package))
    cli('install', bundle, '--accept-permissions')
    result = cli('call', 'rhyven_call', json.dumps({'category':package['name'], 'function':'action_analyze', 'args':{'text':'check'}}))
    assert result['words'] == 0
    print('PASS: parent credentials are absent from the child environment', flush=True)
