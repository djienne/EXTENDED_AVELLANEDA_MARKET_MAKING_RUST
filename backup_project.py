#!/usr/bin/env python3
"""
Project Backup Script

Creates a zip backup of the project, excluding temporary files,
compiled artifacts, and large data directories.
Will overwrite existing backup file if present.
"""

import os
import zipfile
from pathlib import Path
import fnmatch

# Patterns to exclude (directories and files)
EXCLUDE_PATTERNS = [
    # Rust build artifacts
    'target/',
    'target\\',
    'Cargo.lock',  # Optional: include if you want deterministic builds

    # Git
    '.git/',
    '.git\\',
    '.gitignore',

    # IDE and editor files
    '.vscode/',
    '.vscode\\',
    '.idea/',
    '.idea\\',
    '*.swp',
    '*.swo',
    '*.swn',
    '*~',
    '.DS_Store',

    # Python artifacts (if any)
    '__pycache__/',
    '__pycache__\\',
    '*.pyc',
    '*.pyo',
    '*.pyd',
    '.Python',
    'venv/',
    'venv\\',
    'env/',
    'env\\',

    # Data and logs (can be large)
    'data/',
    'data\\',
    '*.log',
    '*.csv',  # Remove if you want to backup CSV data

    # OS files
    'Thumbs.db',
    'desktop.ini',

    # Temporary files
    '*.tmp',
    '*.temp',
    '*.bak',

    # Backup files (don't backup backups!)
    'backup_*.zip',
    'extended_MM.zip',
]

def should_exclude(path, base_path):
    """
    Check if a path should be excluded based on patterns.

    Args:
        path: Path object to check
        base_path: Base project directory Path object

    Returns:
        bool: True if path should be excluded
    """
    # Get relative path
    try:
        rel_path = path.relative_to(base_path)
    except ValueError:
        return True

    rel_path_str = str(rel_path)

    # Check each pattern
    for pattern in EXCLUDE_PATTERNS:
        # Directory patterns (end with /)
        if pattern.endswith('/') or pattern.endswith('\\'):
            pattern_clean = pattern.rstrip('/\\')
            # Check if any part of the path matches
            for part in rel_path.parts:
                if fnmatch.fnmatch(part, pattern_clean):
                    return True
        # File patterns
        else:
            if fnmatch.fnmatch(rel_path_str, pattern) or fnmatch.fnmatch(path.name, pattern):
                return True

    return False

def create_backup(project_dir=None, output_dir=None):
    """
    Create a zip backup of the project.

    Args:
        project_dir: Directory to backup (default: current directory)
        output_dir: Where to save backup (default: same as project_dir)
    """
    # Set default paths
    if project_dir is None:
        project_dir = Path.cwd()
    else:
        project_dir = Path(project_dir)

    if output_dir is None:
        output_dir = project_dir  # Create backup in same directory
    else:
        output_dir = Path(output_dir)

    # Create backup filename (without timestamp - will overwrite existing)
    backup_filename = "extended_MM.zip"
    backup_path = output_dir / backup_filename

    print(f"Creating backup of: {project_dir}")
    print(f"Output file: {backup_path}")
    print(f"Excluding patterns: {len(EXCLUDE_PATTERNS)} patterns")
    print()

    # Statistics
    files_added = 0
    files_excluded = 0
    total_size = 0

    # Create zip file
    with zipfile.ZipFile(backup_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
        # Walk through directory
        for root, dirs, files in os.walk(project_dir):
            root_path = Path(root)

            # Filter out excluded directories (modifies dirs in-place to prevent descent)
            dirs[:] = [d for d in dirs if not should_exclude(root_path / d, project_dir)]

            # Process files
            for file in files:
                file_path = root_path / file

                # Check if file should be excluded
                if should_exclude(file_path, project_dir):
                    files_excluded += 1
                    continue

                # Get relative path for zip archive
                arc_name = file_path.relative_to(project_dir)

                # Add to zip
                try:
                    zipf.write(file_path, arc_name)
                    file_size = file_path.stat().st_size
                    total_size += file_size
                    files_added += 1

                    # Print progress every 10 files
                    if files_added % 10 == 0:
                        print(f"  Added {files_added} files... ({total_size / 1024 / 1024:.2f} MB)")
                except Exception as e:
                    print(f"  Warning: Could not add {arc_name}: {e}")

    # Print summary
    print()
    print("=" * 60)
    print("Backup Complete!")
    print("=" * 60)
    print(f"Files added:     {files_added}")
    print(f"Files excluded:  {files_excluded}")
    print(f"Total size:      {total_size / 1024 / 1024:.2f} MB")
    print(f"Backup location: {backup_path}")
    print(f"Backup size:     {backup_path.stat().st_size / 1024 / 1024:.2f} MB")
    print("=" * 60)

if __name__ == "__main__":
    import sys

    # Allow custom directory as command line argument
    if len(sys.argv) > 1:
        project_dir = sys.argv[1]
    else:
        project_dir = None

    create_backup(project_dir)
