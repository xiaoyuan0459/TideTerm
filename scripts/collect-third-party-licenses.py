#!/usr/bin/env python3
"""Collect upstream license files from the resolved Windows Cargo dependency graph."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest-path', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('Use a new output directory for each license inventory')
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--manifest-path', str(args.manifest_path),
        '--locked', '--format-version', '1', '--filter-platform',
        'x86_64-pc-windows-msvc',
    ]))
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    pending = [metadata['resolve']['root']]
    reachable = set()
    while pending:
        package = pending.pop()
        if package in reachable:
            continue
        reachable.add(package)
        pending.extend(dep['pkg'] for dep in nodes[package]['deps'])
    args.output.mkdir(parents=True)
    inventory = []
    for package in metadata['packages']:
        if package['id'] not in reachable:
            continue
        if package['name'] in {'native', 'tiderm-core', 'tiderm-terminal', 'tiderm-textdoc'}:
            continue
        base = Path(package['manifest_path']).parent
        files = {path for path in base.iterdir() if path.is_file() and
                 path.name.lower().startswith(('license', 'licence', 'copying', 'notice', 'unlicense'))}
        if package.get('license_file'):
            declared = base / package['license_file']
            if declared.is_file():
                files.add(declared)
        destination = args.output / f"{package['name']}-{package['version']}"
        destination.mkdir()
        for path in sorted(files):
            shutil.copyfile(path, destination / path.name)
        record = {
            'name': package['name'], 'version': package['version'],
            'declared_license': package.get('license'),
            'repository': package.get('repository'),
            'license_files': sorted(path.name for path in files),
        }
        (destination / 'package.json').write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
        inventory.append(record)
    missing = [f"{p['name']}@{p['version']}" for p in inventory if not p['license_files']]
    (args.output / 'packages.json').write_text(json.dumps(inventory, indent=2) + '\n', encoding='utf-8')
    print(f'Collected available license files for {len(inventory)} resolved packages')
    if missing:
        print('Packages without standalone license files: ' + ', '.join(missing))
        raise SystemExit('Review upstream licensing before distributing this package')


if __name__ == '__main__':
    main()
