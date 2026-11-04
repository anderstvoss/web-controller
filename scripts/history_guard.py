"""Disclosure scan over every file version and commit identity reachable from any ref.

Making a repository public publishes its whole history, including every branch,
tag and pull-request head, so a scan of the checked-out tree is not enough. This
applies the same content patterns as the pre-commit blockers to every distinct
historical file version, and reports commit author or committer emails that are
not GitHub or bot noreply addresses.

Usage:
    python scripts/history_guard.py [--identities fail|warn|off] [REF ...]

With no REF it scans `--all` (every local branch, tag, remote ref, and any
pull-request heads fetched into refs/). Exit status is 1 when any content
problem is found, or when an identity problem is found with `--identities fail`.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

# Kept in step with the pygrep blockers in .pre-commit-config.yaml, plus
# MAC-shaped hardware addresses, which a tree-only hook set does not catch.
PATTERNS = [
    ('local machine path', re.compile(
        r'(/Users/|/home/[^/\s]+/|/mnt/[A-Za-z]/Users/|[A-Za-z]:\\Users\\|[A-Za-z]:/Users/)')),
    ('ssh config or token', re.compile(
        r'(\bHostName\s+\S+|\bIdentityFile\s+|github_pat_[A-Za-z0-9_]{20,}|\bgh[pousr]_[A-Za-z0-9_]{20,})')),
    ('private key', re.compile(r'-----BEGIN [A-Z ]*PRIVATE KEY-----')),
    ('private storage URI', re.compile(
        r'\b(s3|gs|gcs|azure|wasbs?|abfss?|oss)://[A-Za-z0-9._-]+')),
    ('private network target', re.compile(
        r'\b(host\.docker\.internal|127\.0\.0\.1|10\.\d{1,3}\.\d{1,3}\.\d{1,3}'
        r'|192\.168\.\d{1,3}\.\d{1,3}|172\.(1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b')),
    ('MAC or Bluetooth address', re.compile(r'\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b')),
]

# Files that must spell the patterns out to enforce them.
SKIP_PATHS = {
    '.pre-commit-config.yaml',
    '.githooks/pre-push',
    'scripts/history_guard.py',
    'scripts/tests/test_history_guard.py',
}

NOREPLY_SUFFIXES = ('@users.noreply.github.com',)
NOREPLY_ADDRESSES = {'noreply@github.com', 'noreply@anthropic.com'}


def git(root, *args, data=None):
    return subprocess.run(['git', '-C', str(root), *args], input=data, check=True,
                          capture_output=True).stdout


def content_problems(path, data):
    """Return disclosure problems for one file version."""
    if path in SKIP_PATHS or b'\0' in data[:8192]:
        return []
    text = data.decode('utf-8', errors='replace')
    problems = []
    for label, pattern in PATTERNS:
        for number, line in enumerate(text.splitlines(), 1):
            # Match the transport-test exception in the tree hook, allowing
            # only the loopback bind literal used by this test directory.
            checked = line
            if label == 'private network target' and path.startswith('src/server/tests/'):
                checked = re.sub(r'127\.0\.0\.1', '<test-loopback>', line)
            if pattern.search(checked):
                problems.append(f'{path}:{number}: {label}')
                break
    return problems


def blobs(root, refs):
    """Yield (path, blob id) once per distinct object reachable from refs."""
    seen = set()
    for line in git(root, 'rev-list', '--objects', *refs).decode().splitlines():
        sha, _, path = line.partition(' ')
        if path and sha not in seen:
            seen.add(sha)
            yield path, sha


def content_scan(root, refs):
    entries = list(blobs(root, refs))
    if not entries:
        return []
    kinds = git(root, 'cat-file', '--batch-check=%(objecttype)',
                data=''.join(sha + '\n' for _, sha in entries).encode()).decode().split()
    problems = []
    for (path, sha), kind in zip(entries, kinds):
        if kind == 'blob':
            data = git(root, 'cat-file', 'blob', sha)
            problems.extend(p + f' (blob {sha[:12]})' for p in content_problems(path, data))
    return problems


def identity_scan(root, refs):
    commits = {}
    log = git(root, 'log', '--format=%h%x00%ae%x00%ce', *refs).decode()
    for line in log.splitlines():
        commit, author, committer = line.split('\0')
        for email in {author, committer}:
            if email not in NOREPLY_ADDRESSES and not email.endswith(NOREPLY_SUFFIXES):
                commits.setdefault(email, []).append(commit)
    return [f'commit email {email} in {len(ids)} commit(s): {" ".join(sorted(ids))}'
            for email, ids in sorted(commits.items())]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--identities', choices=('fail', 'warn', 'off'), default='fail')
    parser.add_argument('--root', default='.')
    parser.add_argument('refs', nargs='*')
    args = parser.parse_args(argv)
    root = Path(args.root)
    refs = args.refs or ['--all']

    status = 0
    for problem in content_scan(root, refs):
        print(f'ERROR: {problem}')
        status = 1
    if args.identities != 'off':
        for problem in identity_scan(root, refs):
            level = 'ERROR' if args.identities == 'fail' else 'WARNING'
            print(f'{level}: {problem}')
            if args.identities == 'fail':
                status = 1
    if status:
        print('Use a GitHub noreply identity (git config user.email '
              '<id>+<login>@users.noreply.github.com) and rewrite affected history '
              'before the repository is public.', file=sys.stderr)
    else:
        print('history guard: no disclosure problems in reachable history')
    return status


if __name__ == '__main__':
    sys.exit(main())
