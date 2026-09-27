"""Check source export boundaries before any repository publication."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('engine_export', ROOT / 'scripts/export-public-engine.py')
exporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exporter)


class EngineExportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.source = Path(self.temp.name) / 'source'
        self.source.mkdir()
        (self.source / 'README.md').write_text('Reviewed source\n')
        self.output = Path(self.temp.name) / 'public'

    def test_only_selected_files_and_exact_hashes(self):
        for name in ['state.sqlite3', 'private.pem', 'new-notes.md']:
            (self.source / name).write_text('PRIVATE_SENTINEL')
        exporter.export(self.source, self.output, {'README.md': 'README.md'})
        self.assertEqual({p.name for p in self.output.iterdir()}, {'README.md', 'SOURCE-MANIFEST.json'})
        manifest = json.loads((self.output / 'SOURCE-MANIFEST.json').read_text())
        self.assertEqual(manifest['files'], {'README.md': hashlib.sha256(b'Reviewed source\n').hexdigest()})
        with self.assertRaises(ValueError):
            exporter.export(self.source, self.output, {'README.md': 'README.md'})

    def test_paths_symlinks_and_credentials_fail_before_export(self):
        for destination, source in [('../escape', 'README.md'), ('state.sqlite3', 'README.md'),
                                    ('README.md', '../outside'), ('.git/config', 'README.md')]:
            with self.assertRaises(ValueError):
                exporter.export(self.source, self.output, {destination: source})
            self.assertFalse(self.output.exists())
        path = self.source / 'README.md'
        path.unlink()
        path.symlink_to(ROOT / 'README.md')
        with self.assertRaises(ValueError):
            exporter.export(self.source, self.output, {'README.md': 'README.md'})
        path.unlink()
        path.write_text('-----BEGIN ' + 'PRIVATE KEY-----\nsynthetic fixture')
        with self.assertRaises(ValueError) as failure:
            exporter.export(self.source, self.output, {'README.md': 'README.md'})
        self.assertNotIn('synthetic fixture', str(failure.exception))
        self.assertFalse(self.output.exists())


if __name__ == '__main__':
    unittest.main()
