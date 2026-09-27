"""Image triage must bind evidence to an exact image/package and expire."""
from datetime import date
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location('review', Path(__file__).resolve().parents[1] / 'scripts/review-image-scan.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
identity = 'sha256:' + 'a' * 64
report = {'Metadata': {'ImageID': identity}, 'Results': [{'Type': 'ubuntu', 'Vulnerabilities': [
    {'PkgName': 'linux-libc-dev', 'InstalledVersion': '6.8.0-142.142', 'Severity': 'HIGH', 'VulnerabilityID': 'CVE-test'}]}]}
inventory = {'app': 'repo-documentation-tool', 'image_id': identity, 'version': '6.8.0-142.142', 'header_only': True}
today = date(2026, 9, 27)
assert module.review(report, inventory, today)['passed']
for invalid in ({}, dict(inventory, image_id='sha256:' + 'b' * 64), dict(inventory, header_only=False), dict(inventory, version='another'), dict(inventory, app='another')):
    assert not module.review(report, invalid, today)['passed']
assert not module.review(report, inventory, date(2026, 10, 12))['passed']
report['Results'][0]['Secrets'] = [{}]
assert not module.review(report, inventory, today)['passed']
report['Results'][0]['Secrets'] = []
report['Results'][0]['Vulnerabilities'][0]['PkgName'] = 'some-executable-library'
assert not module.review(report, inventory, today)['passed']
try:
    module.review({}, inventory, today)
except ValueError:
    pass
else:
    raise AssertionError('Incomplete report accepted')
print('PASS: image scan identity, evidence, package scope, expiry, secrets and malformed-report rejection')
