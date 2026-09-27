"""Production scripts and their cache policies must change together."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('stage_site', ROOT / 'scripts/stage-site.py')
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)


class WebsiteStagingTests(unittest.TestCase):
    def tree(self, directory, documentation):
        tree = Path(directory)
        (tree / 'docs.js').write_text(documentation)
        (tree / 'skills.js').write_text("import {docs} from './docs.js';")
        (tree / 'app.js').write_text("import {docs} from './docs.js'; import './skills.js';")
        (tree / 'index.html').write_text('<script type="module" src="app.js"></script>')
        scripts = stage.fingerprint_scripts(tree)
        return tree, scripts

    def test_dependency_change_updates_every_dependent_url(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            old, before = self.tree(a, 'export const docs = 1;')
            new, after = self.tree(b, 'export const docs = 2;')
            self.assertTrue(before.isdisjoint(after))
            self.assertNotEqual((old / 'index.html').read_text(), (new / 'index.html').read_text())
            for name in after:
                data = (new / name).read_bytes()
                self.assertIn(hashlib.sha256(data).hexdigest()[:16], name)
                self.assertNotIn(b"'./docs.js'", data)
                self.assertNotIn(b"'./skills.js'", data)
            self.assertFalse((new / 'app.js').exists())

    def test_cache_rules_do_not_mix_mutable_and_immutable_directives(self):
        with tempfile.TemporaryDirectory() as temporary:
            tree, scripts = self.tree(temporary, 'export const docs = 1;')
            (tree / 'install.sh').write_text('#!/bin/sh\n')
            headers = stage.deployment_headers(tree, scripts)
            wildcard = headers.split('\n/')[0]
            self.assertIn('Cache-Control: no-transform', wildcard)
            self.assertNotIn('no-store', wildcard)
            self.assertNotIn('no-cache', wildcard)
            self.assertIn('/\n  Cache-Control: no-store\n', headers)
            self.assertIn('/install.sh\n  Cache-Control: no-store\n', headers)
            self.assertIn('/releases/*\n  Cache-Control: public, max-age=31536000, immutable\n', headers)
            for name in scripts:
                self.assertIn(f'/{name}\n  Cache-Control: public, max-age=31536000, immutable\n', headers)


if __name__ == '__main__':
    unittest.main()
