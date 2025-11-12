"""
Extended DEX Connector Deployment Script (Rust)

Deploys the Rust library and market making bot to remote VPS using scp with tar compression over SSH.
Cross-platform compatible (Windows, Linux, macOS).

Deployment process:
- Creates compressed tar.gz archive locally (with exclusions)
- Transfers single archive file via scp (efficient single-file transfer)
- Extracts archive on remote server
- Falls back to rsync if available (faster incremental updates)

What gets deployed:
- All Rust source code (src/, examples/, binaries in src/bin/)
- Configuration files (Cargo.toml, Cargo.lock, config.json)
- Python dependencies (requirements.txt, python_sdk-starknet/)
- Python signing script (scripts/sign_order.py)
- Shell scripts (*.sh) - Utility scripts (run_nohup.sh, kill_process.sh, restart_bot.sh)
- Environment file (.env) - WITH API CREDENTIALS
- Docker files (Dockerfile, docker-compose.yml, .dockerignore, DOCKER.md) - ALWAYS INCLUDED
- Documentation (*.md, CLAUDE.md, INSTALL.md)

What gets excluded (NEVER overwrites on remote):
- Git files (.git/, .gitignore)
- Rust build artifacts (target/) - will be compiled on remote server or in Docker
- Historical data (data/) - PROTECTS REMOTE CSV FILES
- CSV files (*.csv) - Never overwrite collected data
- PnL tracking state (pnl_state.json) - Preserves cumulative PnL history
- Log files (*.log, logs/)
- SSH keys (lighter.pem, *.pem) except .env
- IDE settings (.vscode/, .idea/, .claude/)
- Deployment scripts (deploy.py, backup_project.py)
- Temporary files (nul, *.swp, *.swo, .DS_Store)
- Backup archives (backup_*.zip)

After deployment, you can choose either:

OPTION A - Docker Deployment (Recommended):
1. docker-compose build
2. docker-compose up -d
3. docker-compose logs -f

OPTION B - Direct Deployment:
1. pip3 install -r requirements.txt
2. cd python_sdk-starknet && pip3 install -e .
3. cargo build --release
4. ./target/release/market_maker_bot

IMPORTANT: The data/ directory is EXCLUDED to preserve remote historical CSV data.
The bot will create data/ and CSV files automatically on the remote server.

Usage:
    python deploy.py
"""

import os
import sys
import subprocess
import platform
from pathlib import Path

# Configuration
REMOTE_USER = "ubuntu"
REMOTE_HOST = "54.95.246.213"
REMOTE_PATH = "/home/ubuntu/extended_MM"
LOCAL_PATH = "."

# Find SSH key in multiple locations (cross-platform: Windows + WSL)
def find_ssh_key():
    """Find SSH key in Windows or WSL filesystem."""
    possible_paths = [
        os.path.expanduser("~/lighter.pem"),  # WSL/Linux home directory
        "./lighter.pem",  # Current directory (Windows native)
        os.path.join(os.getcwd(), "lighter.pem"),  # Absolute current dir
        "lighter.pem",  # Relative path
    ]

    for path in possible_paths:
        if os.path.exists(path):
            return os.path.abspath(path)

    return None

SSH_KEY = find_ssh_key()

# Files that must always be shipped even if they match an exclusion rule
INCLUDE_ALWAYS = {
    ".env",  # User explicitly requested .env transfer
    "config.json",
    "run_nohup.sh",  # Bot management scripts
    "kill_process.sh",
    "restart_bot.sh",
}

# Files and directories to exclude from deployment
EXCLUDE_PATTERNS = [
    '.git/',
    '.gitignore',
    'target/',  # Rust build artifacts - will be compiled on remote or in Docker
    'data/',  # CRITICAL: Don't overwrite remote historical CSV data
    '*.csv',  # Don't overwrite any CSV files (including data/)
    'pnl_state.json',  # Don't overwrite persistent PnL tracking state
    '*.log',
    'logs/',
    'logs_remote/',  # Don't overwrite remote logs
    '.vscode/',
    '.idea/',
    '.claude/',  # Claude Code settings
    '*.pem',  # SSH keys
    'lighter.pem',
    'nul',  # Windows null device file
    '*.swp',  # Vim swap files
    '*.swo',
    '.DS_Store',  # macOS
    'Thumbs.db',  # Windows
    'deploy.py',  # Don't upload deployment script itself
    'backup_project.py',  # Don't upload backup script
    'backup_*.zip',  # Don't upload backup archives
    'CLAUDE_LONG.md',  # Don't upload long version of docs
]

# Docker files (Dockerfile, docker-compose.yml, .dockerignore, DOCKER.md) are ALWAYS included


def print_header(text):
    """Print a formatted header."""
    print("\n" + "=" * 60)
    print(text.center(60))
    print("=" * 60 + "\n")


def print_success(text):
    """Print success message."""
    print(f"[OK] {text}")


def print_error(text):
    """Print error message."""
    print(f"[ERROR] {text}")


def print_warning(text):
    """Print warning message."""
    print(f"[WARNING] {text}")


def print_info(text):
    """Print info message."""
    print(f"[INFO] {text}")


def check_ssh_key():
    """Check if SSH key exists and set proper permissions."""
    if SSH_KEY is None:
        print_error("SSH key 'lighter.pem' not found in any expected location")
        print("Searched locations:")
        print("  - ~/lighter.pem (WSL/Linux home)")
        print("  - ./lighter.pem (current directory)")
        print("Please ensure lighter.pem is in one of these locations")
        return False

    ssh_key_path = Path(SSH_KEY)
    print_success(f"Found SSH key: {SSH_KEY}")

    # Set proper permissions (Unix-like systems only)
    if platform.system() != "Windows":
        try:
            os.chmod(ssh_key_path, 0o600)
            print_success(f"SSH key permissions set to 600")
        except Exception as e:
            print_warning(f"Could not set SSH key permissions: {e}")

    return True


def test_ssh_connection():
    """Test SSH connection to remote server."""
    print_info("Testing SSH connection (may take up to 60 seconds)...")

    cmd = [
        "ssh",
        "-i", SSH_KEY,
        "-o", "ConnectTimeout=30",
        "-o", "StrictHostKeyChecking=no",
        f"{REMOTE_USER}@{REMOTE_HOST}",
        "echo 'Connection successful'"
    ]

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=60
        )

        if result.returncode == 0:
            print_success("SSH connection successful")
            return True
        else:
            print_error("Cannot connect to remote server")
            print(f"Host: {REMOTE_USER}@{REMOTE_HOST}")
            print(f"Key: {SSH_KEY}")
            if result.stderr:
                print(f"Error: {result.stderr}")
            return False
    except subprocess.TimeoutExpired:
        print_error("SSH connection timed out after 60 seconds")
        return False
    except FileNotFoundError:
        print_error("SSH client not found. Please install OpenSSH.")
        return False
    except Exception as e:
        print_error(f"SSH test failed: {e}")
        return False


def create_remote_directory():
    """Create remote directory if it doesn't exist."""
    print_info("Ensuring remote directory exists...")

    cmd = [
        "ssh",
        "-i", SSH_KEY,
        "-o", "ConnectTimeout=30",
        "-o", "StrictHostKeyChecking=no",
        f"{REMOTE_USER}@{REMOTE_HOST}",
        f"mkdir -p {REMOTE_PATH}"
    ]

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=60
        )
        if result.returncode == 0:
            print_success(f"Remote directory ready: {REMOTE_PATH}")
            return True
        else:
            print_error(f"Failed to create remote directory: {result.stderr}")
            return False
    except subprocess.TimeoutExpired:
        print_error("Remote directory creation timed out after 60 seconds")
        return False
    except Exception as e:
        print_error(f"Error creating remote directory: {e}")
        return False


def check_rsync_available():
    """Check if rsync is available."""
    try:
        result = subprocess.run(
            ["rsync", "--version"],
            capture_output=True,
            text=True
        )
        return result.returncode == 0
    except FileNotFoundError:
        return False


def deploy_with_rsync():
    """Deploy using rsync (faster, incremental)."""
    print_info("Deploying using rsync (incremental sync)...")

    # Build exclude arguments
    exclude_args = []
    for pattern in EXCLUDE_PATTERNS:
        exclude_args.extend(["--exclude", pattern])

    cmd = [
        "rsync",
        "-avz",
        "--progress",
        "--delete",
        "-e", f"ssh -i {SSH_KEY} -o StrictHostKeyChecking=no -o ConnectTimeout=30",
        *exclude_args,
        f"{LOCAL_PATH}/",
        f"{REMOTE_USER}@{REMOTE_HOST}:{REMOTE_PATH}/"
    ]

    try:
        result = subprocess.run(cmd, timeout=300)
        return result.returncode == 0
    except subprocess.TimeoutExpired:
        print_error("Rsync deployment timed out after 5 minutes")
        return False
    except Exception as e:
        print_error(f"Rsync deployment failed: {e}")
        return False


def deploy_with_scp():
    """Deploy using scp with tar compression (efficient single-file transfer)."""
    print_info("Deploying using scp with tar compression...")

    import tempfile
    import tarfile

    # Create temporary tar file
    with tempfile.NamedTemporaryFile(suffix='.tar.gz', delete=False) as tmp_tar:
        tar_path = tmp_tar.name

    try:
        print_info("Creating compressed archive...")

        # Create tar archive with exclusions
        def filter_exclude(tarinfo):
            """Filter function to exclude files matching patterns."""
            # Get relative path
            path_str = tarinfo.name
            base_name = os.path.basename(path_str)

            # Never exclude files explicitly marked for inclusion
            if base_name in INCLUDE_ALWAYS:
                return tarinfo

            for pattern in EXCLUDE_PATTERNS:
                if pattern.endswith('/'):
                    # Directory pattern - check if directory name matches
                    dir_name = pattern.rstrip('/')
                    if dir_name in path_str.split('/'):
                        return None
                elif '*' in pattern:
                    # Wildcard pattern
                    import fnmatch
                    if fnmatch.fnmatch(path_str, pattern) or fnmatch.fnmatch(base_name, pattern):
                        return None
                else:
                    # Exact filename match
                    if pattern == base_name or pattern in path_str:
                        return None

            return tarinfo

        # Create the archive
        files_added = []
        with tarfile.open(tar_path, 'w:gz') as tar:
            def filter_and_track(tarinfo):
                result = filter_exclude(tarinfo)
                if result is not None:
                    files_added.append(tarinfo.name)
                return result

            tar.add(LOCAL_PATH, arcname='extended_mm', filter=filter_and_track)

        # Get archive size
        archive_size_mb = os.path.getsize(tar_path) / (1024 * 1024)
        print_success(f"Archive created: {archive_size_mb:.2f} MB ({len(files_added)} files)")

        # List shell scripts that were included
        sh_files = [f for f in files_added if f.endswith('.sh')]
        if sh_files:
            print_success(f"Shell scripts included: {', '.join([os.path.basename(f) for f in sh_files])}")
        else:
            print_warning("No .sh files found in archive!")

        # Transfer archive to remote
        print_info("Transferring archive to remote server...")

        scp_cmd = [
            "scp",
            "-i", SSH_KEY,
            "-o", "StrictHostKeyChecking=no",
            "-o", "ConnectTimeout=30",
            tar_path,
            f"{REMOTE_USER}@{REMOTE_HOST}:/tmp/extended_mm_deploy.tar.gz"
        ]

        result = subprocess.run(scp_cmd, capture_output=True, text=True, timeout=120)
        if result.returncode != 0:
            print_error(f"Failed to transfer archive: {result.stderr}")
            return False

        print_success("Archive transferred successfully")

        # Extract archive on remote server
        print_info("Extracting archive on remote server...")

        extract_cmd = [
            "ssh",
            "-i", SSH_KEY,
            "-o", "StrictHostKeyChecking=no",
            "-o", "ConnectTimeout=30",
            f"{REMOTE_USER}@{REMOTE_HOST}",
            f"cd {REMOTE_PATH} && tar -xzf /tmp/extended_mm_deploy.tar.gz --strip-components=1 && rm /tmp/extended_mm_deploy.tar.gz"
        ]

        result = subprocess.run(extract_cmd, capture_output=True, text=True, timeout=60)
        if result.returncode != 0:
            print_error(f"Failed to extract archive: {result.stderr}")
            return False

        print_success("Deployment completed successfully")
        return True

    except Exception as e:
        print_error(f"SCP deployment failed: {e}")
        return False
    finally:
        # Clean up local tar file
        try:
            if os.path.exists(tar_path):
                os.unlink(tar_path)
        except:
            pass


def display_next_steps():
    """Display next steps after successful deployment."""
    print("\n" + "=" * 60)
    print("Deployment completed successfully!".center(60))
    print("=" * 60 + "\n")

    print("Next steps:\n")
    print(f"1. SSH into the server:")
    print(f"   ssh -i {SSH_KEY} {REMOTE_USER}@{REMOTE_HOST}\n")

    print(f"2. Navigate to the project directory:")
    print(f"   cd {REMOTE_PATH}\n")

    print("3. Verify .env file was transferred:")
    print("   cat .env")
    print("   Required: API_KEY, STARK_PUBLIC, STARK_PRIVATE, VAULT_NUMBER, EXTENDED_ENV\n")

    print("═══════════════════════════════════════════════════════════")
    print("OPTION A: Docker Deployment (Recommended)")
    print("═══════════════════════════════════════════════════════════\n")

    print("4. Install Docker and Docker Compose (if not already installed):")
    print("   curl -fsSL https://get.docker.com -o get-docker.sh")
    print("   sudo sh get-docker.sh")
    print("   sudo usermod -aG docker $USER")
    print("   sudo apt-get update && sudo apt-get install -y docker-compose-plugin")
    print("   # Log out and back in for group changes to take effect\n")

    print("5. Build the Docker image:")
    print("   docker-compose build")
    print("   # This will take 5-10 minutes on first build\n")

    print("6. Start the bot:")
    print("   docker-compose up -d\n")

    print("7. View logs:")
    print("   docker-compose logs -f\n")

    print("8. Stop the bot (CRITICAL - cancels all orders):")
    print("   docker-compose stop\n")

    print("═══════════════════════════════════════════════════════════")
    print("OPTION B: Direct Deployment")
    print("═══════════════════════════════════════════════════════════\n")

    print("4. Install Rust (if not already installed):")
    print("   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh")
    print("   source $HOME/.cargo/env\n")

    print("5. Install Python dependencies (following INSTALL.md):")
    print("   pip3 install -r requirements.txt")
    print("   cd python_sdk-starknet && pip3 install -e . && cd ..\n")

    print("6. Build the project:")
    print("   cargo build --release\n")

    print("7. Run the market maker bot:")
    print("   ./target/release/market_maker_bot")
    print("   # OR using helper scripts:")
    print("   chmod +x *.sh")
    print("   ./run_nohup.sh          # Start in background\n")

    print("8. Manage the bot:")
    print("   ./kill_process.sh       # Stop gracefully (cancels orders)")
    print("   ./restart_bot.sh        # Restart (stop + start)")
    print("   tail -f output.log      # View logs\n")

    print("9. CRITICAL - Always use graceful shutdown:")
    print("   NEVER use 'kill -9' - orders won't be cancelled")
    print("   Use ./kill_process.sh or Ctrl+C only\n")

    print("\n" + "=" * 60)
    print("IMPORTANT SAFETY NOTES:")
    print("=" * 60)
    print("• .env file WAS deployed with credentials - verify it's correct")
    print("• data/ directory is EXCLUDED - remote CSV files are PROTECTED")
    print("• The bot will create data/ and collect CSV files automatically")
    print("• NEVER use 'kill -9' - orders won't be cancelled")
    print("• Always use graceful shutdown (Ctrl+C or SIGINT or docker-compose stop)")
    print("• Set trading_enabled: false in config.json to collect data without trading")
    print("• Monitor PnL and positions regularly")


def main():
    """Main deployment function."""
    print_header("Extended DEX Connector (Rust) - Remote Deployment Script")

    # Step 1: Check SSH key
    if not check_ssh_key():
        sys.exit(1)

    # Step 2: Test SSH connection
    if not test_ssh_connection():
        sys.exit(1)

    # Step 3: Create remote directory
    if not create_remote_directory():
        sys.exit(1)

    # Step 4: Display deployment details
    print("\nDeployment Details:")
    print(f"  Local path:  {LOCAL_PATH}")
    print(f"  Remote host: {REMOTE_USER}@{REMOTE_HOST}")
    print(f"  Remote path: {REMOTE_PATH}")
    print(f"  SSH key:     {SSH_KEY}")
    print()

    # Check if .env exists locally
    env_file = Path(".env")
    if env_file.exists():
        print_success(".env file found - WILL BE DEPLOYED with credentials")
        print_warning("Ensure .env contains: API_KEY, STARK_PUBLIC, STARK_PRIVATE, VAULT_NUMBER")
    else:
        print_warning(".env file not found locally")
        print_info("You will need to create .env on the remote server before running the bot")
    print()

    # Check if data directory exists locally
    data_dir = Path("data")
    if data_dir.exists() and data_dir.is_dir():
        print_success("data/ directory found locally - WILL BE EXCLUDED from deployment")
        print_info("Remote CSV files and historical data will be PRESERVED")
    else:
        print_info("No local data/ directory (will be created automatically on remote)")
    print()

    # Check Docker files
    docker_files = ['Dockerfile', 'docker-compose.yml', '.dockerignore']
    missing_files = [f for f in docker_files if not Path(f).exists()]
    if missing_files:
        print_warning(f"Missing Docker files: {', '.join(missing_files)}")
        print_info("Docker deployment won't be available on remote (direct deployment still works)")
    else:
        print_success("Docker files found - will be deployed")
    print()

    # Check shell scripts
    shell_scripts = ['run_nohup.sh', 'kill_process.sh', 'restart_bot.sh']
    existing_scripts = [f for f in shell_scripts if Path(f).exists()]
    if existing_scripts:
        print_success(f"Shell scripts found: {', '.join(existing_scripts)}")
        print_info("These scripts will be deployed and can be used for bot management")
    else:
        print_warning("No shell scripts found in current directory")
    print()

    # Step 5: Deploy (auto-confirmed)
    success = False

    # Use scp with tar compression (efficient single-file transfer)
    # Falls back to rsync if available for incremental updates
    if check_rsync_available():
        print_info("rsync detected - using for incremental sync")
        success = deploy_with_rsync()
    else:
        success = deploy_with_scp()

    # Step 6: Display results
    if success:
        display_next_steps()
    else:
        print("\n" + "=" * 60)
        print("Deployment failed!".center(60))
        print("=" * 60 + "\n")
        sys.exit(1)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n")
        print_warning("Deployment interrupted by user")
        sys.exit(1)
    except Exception as e:
        print_error(f"Unexpected error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
