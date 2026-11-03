"""Tests for scripts/history_guard.py.

Address-shaped values are assembled at runtime so this file never contains a
literal one, matching the repository's own disclosure rules.
"""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import history_guard  # noqa: E402

MAC_SHAPED = ':'.join(['0a'] * 6)
PRIVATE_IP = '.'.join(['192', '168', '0', '1'])
HOME_PATH = '/' + 'home' + '/someone/project'
PERSONAL_EMAIL = 'someone' + '@' + 'example.com'
NOREPLY_EMAIL = '1+someone' + '@' + 'users.noreply.github.com'


def clean_env(**extra):
    """Drop hook-local GIT_* variables so nested git finds the temp repository."""
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
    env.update(extra)
    return env


def run(root, *args, env=None):
    subprocess.run(['git', '-C', str(root), *args], check=True, capture_output=True,
                   env=env or clean_env())


def commit(root, name, text, email):
    (root / name).write_text(text)
    run(root, 'add', name)
    run(root, '-c', f'user.email={email}', '-c', 'user.name=t', 'commit', '-q', '-m', name,
        env=clean_env(GIT_COMMITTER_EMAIL=email, GIT_COMMITTER_NAME='t'))


class ContentProblems(unittest.TestCase):
    def test_flags_each_pattern(self):
        for text, label in [(MAC_SHAPED, 'MAC'), (PRIVATE_IP, 'private network'),
                            (HOME_PATH, 'local machine path')]:
            problems = history_guard.content_problems('notes.md', text.encode())
            self.assertEqual(len(problems), 1, text)
            self.assertIn(label, problems[0])

    def test_clean_and_skipped_files(self):
        self.assertEqual(history_guard.content_problems('a.md', b'version = "10.2.1"\n'), [])
        self.assertEqual(history_guard.content_problems('.pre-commit-config.yaml',
                                                        MAC_SHAPED.encode()), [])
        self.assertEqual(history_guard.content_problems('a.bin', b'\0' + MAC_SHAPED.encode()), [])


class HistoryScan(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        run(self.root, 'init', '-q', '-b', 'main')

    def tearDown(self):
        self.tmp.cleanup()

    def test_removed_content_still_fails(self):
        commit(self.root, 'a.md', MAC_SHAPED + '\n', NOREPLY_EMAIL)
        commit(self.root, 'a.md', 'clean\n', NOREPLY_EMAIL)
        self.assertEqual(history_guard.main(['--root', str(self.root)]), 1)

    def test_identity_modes(self):
        commit(self.root, 'a.md', 'clean\n', PERSONAL_EMAIL)
        self.assertEqual(history_guard.main(['--root', str(self.root)]), 1)
        self.assertEqual(history_guard.main(['--root', str(self.root), '--identities', 'warn']), 0)
        self.assertEqual(history_guard.main(['--root', str(self.root), '--identities', 'off']), 0)

    def test_clean_noreply_history_passes(self):
        commit(self.root, 'a.md', 'clean\n', NOREPLY_EMAIL)
        self.assertEqual(history_guard.main(['--root', str(self.root)]), 0)


if __name__ == '__main__':
    unittest.main()
