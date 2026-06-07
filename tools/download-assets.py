#!/usr/bin/env python3
import os
import sys
import urllib.request
import hashlib
from pathlib import Path

# URLs and expected SHA-256 hashes of the pre-compiled base assets
ASSETS = {
    "systemd-boot-efi_252.39-1~deb12u2_amd64.deb": {
        "url": "https://snapshot.debian.org/archive/debian/20260503T204801Z/pool/main/s/systemd/systemd-boot-efi_252.39-1~deb12u2_amd64.deb",
        "sha256": "8f2b81bdcfafc466882a0cae460272dbf10d8d97c6a3c1c6ea33038c155a2f5e"
    },
    "busybox-static_1.35.0-4+deb12u1+b1_amd64.deb": {
        "url": "https://snapshot.debian.org/archive/debian/20260508T024741Z/pool/main/b/busybox/busybox-static_1.35.0-4%2Bdeb12u1%2Bb1_amd64.deb",
        "sha256": "3d3fdbe91d4660c873e14b092c213fe81c1da6362daa236eb25d0171eb108744"
    },
    "linux-image-6.1.0-49-amd64_6.1.174-1_amd64.deb": {
        "url": "https://snapshot.debian.org/archive/debian/20260530T025123Z/pool/main/l/linux-signed-amd64/linux-image-6.1.0-49-amd64_6.1.174-1_amd64.deb",
        "sha256": "42e9d3062c94978b92193cb63da7356e5dec01fbece40724ca46ad6b16e8e456"
    },
    "ubuntu-base-24.04.4-base-amd64.tar.gz": {
        "url": "https://cdimage.ubuntu.com/ubuntu-base/releases/24.04.4/release/ubuntu-base-24.04.4-base-amd64.tar.gz",
        "sha256": "c1e67ef7b17a6300e136118bd1dc04725009cb376c1aad10abcf8cd453628d58"
    }
}

def get_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()

def download_with_progress(url, dest):
    print(f"Downloading {url} to {dest}...")
    def report(count, block_size, total_size):
        if total_size > 0:
            percent = count * block_size * 100 // total_size
            percent = min(percent, 100)
            sys.stdout.write(f"\r  Progress: {percent}%")
            sys.stdout.flush()
    urllib.request.urlretrieve(url, dest, reporthook=report)
    sys.stdout.write("\n")
    sys.stdout.flush()

def main():
    repo_root = Path(__file__).resolve().parent.parent
    downloads_dir = repo_root / "build" / "tmp_downloads"
    downloads_dir.mkdir(parents=True, exist_ok=True)
    
    for filename, info in ASSETS.items():
        dest = downloads_dir / filename
        url = info["url"]
        
        # Check if exists and verify hash
        if dest.exists():
            actual_hash = get_sha256(dest)
            if actual_hash == info["sha256"]:
                print(f"{filename} already exists and is verified.")
                continue
            else:
                print(f"Hash mismatch for {filename}. Redownloading...")
                dest.unlink()

        try:
            download_with_progress(url, dest)
        except Exception as e:
            print(f"Error downloading {filename}: {e}", file=sys.stderr)
            if dest.exists():
                dest.unlink()
            sys.exit(1)
            
        actual_hash = get_sha256(dest)
        if actual_hash != info["sha256"]:
            print(f"Error: downloaded {filename} hash mismatch! Expected {info['sha256']}, got {actual_hash}", file=sys.stderr)
            dest.unlink()
            sys.exit(1)
        print(f"{filename} successfully downloaded and verified.")
        
if __name__ == "__main__":
    main()
