"""Public app export must never inherit platform files, symlinks or private references."""
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('app_export', ROOT / 'scripts/export-public-apps.py')
exporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exporter)


class AppExportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='rhyven-public-export-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'source'
        self.root.mkdir()
        for relative in exporter.selected_files().values():
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        self.output = Path(self.temp.name) / 'export'

    def test_only_reviewed_files_and_hash_inventory(self):
        for relative in ('.git/config', '.release-signing/private.pem',
                         'crates/core/src/lib.rs', 'apps/messaging/new-private-notes.md',
                         'apps/messaging/.env', 'catalog/state.sqlite3'):
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('PRIVATE_SENTINEL')
        exporter.export(self.root, self.output)
        actual = {str(p.relative_to(self.output)) for p in self.output.rglob('*') if p.is_file()}
        self.assertEqual(actual, set(exporter.selected_files()) | {'SOURCE-MANIFEST.json'})
        for p in self.output.rglob('*'):
            if p.is_file():
                self.assertNotIn('PRIVATE_SENTINEL', p.read_text())
        inventory = json.loads((self.output / 'SOURCE-MANIFEST.json').read_text())
        self.assertEqual(set(inventory['files']), set(exporter.selected_files()))

    def test_symlink_rejected(self):
        p = self.root / 'apps/messaging/main.py'
        p.unlink()
        p.symlink_to(ROOT / 'crates/core/src/lib.rs')
        with self.assertRaisesRegex(ValueError, 'regular app file'):
            exporter.export(self.root, self.output)
        self.assertFalse(self.output.exists())

    def test_private_reference_rejected_without_echoing_it(self):
        p = self.root / 'apps/messaging/README.md'
        p.write_text('Development: https://github.com/rhyven-ai/rhyven-development-archive/actions/runs/123')
        with self.assertRaises(ValueError) as failure:
            exporter.export(self.root, self.output)
        self.assertNotIn('actions/runs/123', str(failure.exception))
        self.assertFalse(self.output.exists())


if __name__ == '__main__':
    unittest.main()
