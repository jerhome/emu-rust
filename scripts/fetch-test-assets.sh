#!/usr/bin/env bash
# Fetch the test assets used by the ignored integration tests (ADR 0007):
#   - 240p Test Suite (Mega Drive) 1.21, GPL-2.0 homebrew, SHA256 pinned
#   - genesis_plus_gx libretro core, libretro nightly, hash recorded on first download (TOFU)
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/fetch-test-assets.sh [--refresh-cores] [--help]

Downloads test assets into tests/roms and tests/cores (both ignored by git).
Idempotent: files already present and matching their hash are not downloaded again.

  --refresh-cores  re-download the core and record its new SHA256 (TOFU)
  --help           show this help

Requires: curl, unzip, sha256sum.
USAGE
}

refresh_cores=0
for arg in "$@"; do
    case "$arg" in
        --refresh-cores) refresh_cores=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $arg" >&2; usage >&2; exit 2 ;;
    esac
done

for tool in curl unzip sha256sum; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
roms_dir="$root/tests/roms"
cores_dir="$root/tests/cores"
mkdir -p "$roms_dir" "$cores_dir"

ROM_URL='https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download'
ROM_ZIP_SHA256='2054524a7f8695d8df7f4cf1a773af6b26d4c7fe7f74d8f115fab2641a5a9dc5'
ROM_ZIP_NAME='240pSuite-GenesisMD-1.21.zip'
ROM_NAME='240pSuite-1.21.bin'
ROM_SHA256='1776009ab324c18e4f046b4781fed11b897281f407ec04f2ac43649bcebae28f'

CORE_URL='https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip'
CORE_NAME='genesis_plus_gx_libretro.so'

MAX_UNZIPPED_BYTES=$((128 * 1024 * 1024))

tmp_dirs=()
part_files=()
cleanup() {
    for d in "${tmp_dirs[@]}"; do rm -rf "$d"; done
    for f in "${part_files[@]}"; do rm -f "$f"; done
}
trap cleanup EXIT

sha_of() { sha256sum "$1" | cut -d' ' -f1; }

# download_zip URL DEST_ZIP
download_zip() {
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --location --silent --show-error \
        --retry 3 --connect-timeout 15 --max-time 300 --max-filesize 64M -o "$2" "$1"
}

# extract_one ZIP MEMBER DEST : unzip into a fresh temp dir, move only the expected file.
extract_one() {
    local zip="$1" member="$2" dest="$3" work
    work="$(mktemp -d)"
    tmp_dirs+=("$work")
    # Refuse zip bombs: check the total uncompressed size announced by the archive.
    local total
    total="$(unzip -Zt "$zip" | awk '{print $3}' | head -n 1)"
    case "$total" in ''|*[!0-9]*) echo "error: cannot read archive size" >&2; exit 1 ;; esac
    [ "$total" -le "$MAX_UNZIPPED_BYTES" ] \
        || { echo "error: archive too large once extracted ($total bytes)" >&2; exit 1; }
    unzip -q -o "$zip" -d "$work"
    local found
    found="$(find "$work" -type f -name "$member" | head -n 1)"
    [ -n "$found" ] || { echo "error: $member not found in archive" >&2; exit 1; }
    mv -f "$found" "$dest"
}

# ---- ROM (pinned); the zip is kept for the archive-loading tests ----
rom_path="$roms_dir/$ROM_NAME"
zip_path="$roms_dir/$ROM_ZIP_NAME"
if [ -f "$zip_path" ] && [ "$(sha_of "$zip_path")" = "$ROM_ZIP_SHA256" ]; then
    echo "ROM zip ok: $zip_path"
else
    echo "Downloading $ROM_ZIP_NAME ..."
    part_files+=("$zip_path.part")
    download_zip "$ROM_URL" "$zip_path.part"
    if [ "$(sha_of "$zip_path.part")" != "$ROM_ZIP_SHA256" ]; then
        rm -f "$zip_path.part"
        echo "error: SHA256 mismatch for ROM zip" >&2
        exit 1
    fi
    mv -f "$zip_path.part" "$zip_path"
    echo "ROM zip installed: $zip_path"
fi
if [ -f "$rom_path" ] && [ "$(sha_of "$rom_path")" = "$ROM_SHA256" ]; then
    echo "ROM ok: $rom_path"
else
    part_files+=("$rom_path.part")
    extract_one "$zip_path" "$ROM_NAME" "$rom_path.part"
    if [ "$(sha_of "$rom_path.part")" != "$ROM_SHA256" ]; then
        rm -f "$rom_path.part"
        echo "error: SHA256 mismatch for $ROM_NAME" >&2
        exit 1
    fi
    mv -f "$rom_path.part" "$rom_path"
    echo "ROM installed: $rom_path"
fi

# ---- Core (TOFU) ----
core_path="$cores_dir/$CORE_NAME"
hash_path="$core_path.sha256"
if [ "$refresh_cores" = 0 ] && [ -f "$core_path" ] && [ -f "$hash_path" ]; then
    expected="$(cut -d' ' -f1 "$hash_path")"
    [ "$(sha_of "$core_path")" = "$expected" ] \
        || { echo "error: $CORE_NAME does not match $hash_path (use --refresh-cores to replace)" >&2; exit 1; }
    echo "Core ok: $core_path ($expected)"
else
    echo "Downloading $CORE_NAME ..."
    work="$(mktemp -d)"; tmp_dirs+=("$work")
    download_zip "$CORE_URL" "$work/core.zip"
    extract_one "$work/core.zip" "$CORE_NAME" "$work/$CORE_NAME"
    # Copy next to the destination so the final mv is an atomic same-filesystem rename.
    part_files+=("$core_path.part")
    cp -f "$work/$CORE_NAME" "$core_path.part"
    mv -f "$core_path.part" "$core_path"
    sum="$(sha_of "$core_path")"
    echo "$sum  $CORE_NAME" > "$hash_path"
    echo "Core installed: $core_path (sha256 $sum, recorded as trusted on first use)"
fi
