"""Build the website's read-only catalog from local, validated app manifests."""
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'website/data/catalog.json'
FILES = [*sorted((ROOT / 'catalog').glob('*.json')), ROOT / 'apps/repo-documentation-tool/app.json', ROOT / 'apps/messaging/app.json']
AVAILABILITY = json.loads((ROOT / 'website/data/availability.json').read_text())
APPS = []
for file in FILES:
    p = json.loads(file.read_text())
    execution = p.get('execution', {})
    kind = 'declarative' if execution.get('driver', 'declarative') == 'declarative' else ('service' if execution.get('mode') == 'service' else 'container')
    APPS.append(dict(id=p['name'], name=p['name'].split('/')[1], version=p['version'], publisher=p['publisher'], display_name=p.get('display_name',p['name']), description=p['description'], kind=kind, permissions=p['permissions'], guide=p['guide'], objects=list(p['objects']), actions=[dict(name=k, description=v.get('description',''), input=v['input']) for k,v in p['actions'].items()], source=str(file.relative_to(ROOT)), registry=AVAILABILITY['registry'] if AVAILABILITY['apps'].get(p['name']) == p['version'] else None))
version = re.search(r'\[workspace.package\]\s*version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text())[1]
OUT.write_text(json.dumps(dict(runtime_version=version, source='local manifests', apps=APPS), indent=2)+'\n')
print(f'Wrote {len(APPS)} app descriptions to {OUT.relative_to(ROOT)}')
