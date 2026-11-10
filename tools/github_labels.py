"""Sets the labels of the issues on GitHub from .github/labels.json, with the gh command:
labels that are missing are made, and those there get the colour and description of the file.
Labels that are not in the file are left alone.

    python3 tools/github_labels.py [owner/repo]

Without a repository, the one of the folder is used. Needs gh, logged in with the right to
change the repository.
"""
import json, os, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    with open(os.path.join(ROOT, '.github', 'labels.json'), encoding='utf-8') as f:
        labels = json.load(f)
    repo = ['--repo', sys.argv[1]] if len(sys.argv) > 1 else []
    for label in labels:
        cmd = ['gh', 'label', 'create', label['name'], '--color', label['color'], '--description', label['description'], '--force'] + repo
        done = subprocess.run(cmd, capture_output=True, text=True)
        print(('ok    ' if done.returncode == 0 else 'error ') + label['name'] + ('' if done.returncode == 0 else ': ' + done.stderr.strip()))


if __name__ == '__main__':
    main()
