#!/usr/bin/env python3
"""Fail release on secrets/high CVEs, retaining a narrowly evidenced header-only assessment."""
import argparse
from datetime import date
import json
from pathlib import Path
import re


def review(report, inventory, today=None):
    today = today or date.today()
    identity = report.get('Metadata', {}).get('ImageID', '')
    if not re.fullmatch(r'sha256:[0-9a-f]{64}', identity) or not isinstance(report.get('Results'), list) or not report['Results']:
        raise ValueError('Expected a complete container scan with image identity and results')
    # Exact package/version; expires so upgrades/new evidence require another review.
    paths = inventory.get('owned_files', [])
    header_paths = (
        isinstance(paths, list) and bool(paths)
        and all(isinstance(p, str) and '..' not in p.split('/')
                and ((p.startswith('/usr/include/') and p.endswith('.h'))
                     or p.startswith('/usr/share/doc/linux-libc-dev/'))
                for p in paths)
    )
    headers_verified = (
        today <= date(2026, 10, 11)
        and inventory.get('app') == 'repo-documentation-tool'
        and inventory.get('image_id') == report.get('Metadata', {}).get('ImageID')
        and inventory.get('version') == '6.8.0-146.146'
        and inventory.get('header_only') is True
        and header_paths
    )
    findings, reviewed, secrets, lower = [], [], 0, []
    for result in report.get('Results', []):
        secrets += len(result.get('Secrets', []))
        for v in result.get('Vulnerabilities', []):
            item = {k: v.get(k) for k in ('VulnerabilityID', 'PkgName', 'InstalledVersion', 'FixedVersion', 'Severity')}
            if (headers_verified and result.get('Type') == 'ubuntu'
                    and v['PkgName'] == 'linux-libc-dev' and v['InstalledVersion'] == inventory['version']
                    and 'kernel' in v.get('Description', '').lower()):
                reviewed.append(item)
            elif v['Severity'] in ('HIGH', 'CRITICAL'):
                findings.append(item)
            else:
                lower.append(item)
    return {
        'passed': not findings and not secrets,
        'blocking_vulnerabilities': findings,
        'secret_findings': secrets,
        'reviewed_header_findings': reviewed,
        'review_reason': 'Exact package contains UAPI headers/docs only; no kernel executable. Host kernel remains operator responsibility. Review expires 2026-10-11.' if reviewed else None,
        'remaining_low_medium_unknown': lower,
    }


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--inventory', type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    result = review(json.loads(args.report.read_text()), json.loads(args.inventory.read_text()) if args.inventory else {})
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: len(v) if isinstance(v, list) else v for k, v in result.items()}, indent=2))
    raise SystemExit(not result['passed'])
