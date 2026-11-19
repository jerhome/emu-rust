#!/usr/bin/env bash
# Fetch the test assets used by the ignored integration tests (ADR 0007):
#   - 240p Test Suite (Mega Drive) 1.21, GPL-2.0 homebrew, SHA256 pinned
#   - 240p Test Suite (SNES) 1.02, GPL-2.0 homebrew (ArtemioUrbina/240pTestSuite), SHA256 pinned
#   - Sub Rescue 0.3 (haroldo-ok, MIT), Master System homebrew, SHA256 pinned
#   - Waternet 1.0 (joyrider3774, MIT), native Game Gear homebrew, SHA256 pinned
#   - genesis_plus_gx and snes9x libretro cores, libretro nightly, hash recorded on first download (TOFU)
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/fetch-test-assets.sh [--refresh-cores] [--help]

Downloads test assets into tests/roms and tests/cores (both ignored by git).
Idempotent: files already present and matching their hash are not downloaded again.

  --refresh-cores  re-download the cores and record its new SHA256 (TOFU)
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

SNES_ZIP_URL='https://sourceforge.net/projects/testsuite240p/files/OldFiles/SNES_SFC/240pSuite-SNES-1.02.zip/download'
SNES_ZIP_SHA256='6a0c79b68f33d3df1256f0c4db7dd32dfab41190726a6b35b76061b4edf7054b'
SNES_ZIP_NAME='240pSuite-SNES-1.02.zip'
SNES_MEMBER='240pSuite.sfc'
SNES_NAME='240pSuite-SNES-1.02.sfc'
SNES_SHA256='bf73e9f4a55da8c66379f1f6126fc14d55c56adbe007dbfebeec77475eca6949'

# Sub Rescue: https://github.com/haroldo-ok/sub-rescue (LICENSE = MIT)
SR_ZIP_URL='https://github.com/haroldo-ok/sub-rescue/releases/download/v0.3/sub_rescue-0.3.zip'
SR_ZIP_SHA256='fabe39d19f3280af06d3bc3058368d5016c9ae7a796319a2f1ecfe89e64b3f69'
SR_ZIP_NAME='sub_rescue-0.3.zip'
SR_MEMBER='sub_rescue-0.3.sms'
SR_NAME='sub_rescue-0.3.sms'
SR_SHA256='5d8013936eab3390826692f8fcc974e08e535371a97c47953b8d003b76866741'

# Waternet: https://github.com/joyrider3774/waternet (LICENSE = MIT, also shipped in the zip)
GG_ZIP_URL='https://github.com/joyrider3774/waternet/releases/download/v1.0/Waternet.Sega.Game.Gear.zip'
GG_ZIP_SHA256='16a06f51168717c252b08687abe6dd5980a4090d5cbe228f5e01d4ed28579fee'
GG_ZIP_NAME='Waternet-GameGear-1.0.zip'
GG_MEMBER='waternet.gg'
GG_NAME='waternet-1.0.gg'
GG_SHA256='ab9f288e2b556263cddd7856deb67269cfe08ac1e6729ca9e44c5e2c19bb1d8c'

CORE_BASE_URL='https://buildbot.libretro.com/nightly/linux/x86_64/latest'
CORES=(genesis_plus_gx_libretro.so snes9x_libretro.so)

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

# fetch_rom ZIP_URL ZIP_SHA256 ZIP_NAME MEMBER ROM_NAME ROM_SHA256
# The zip is kept next to the ROM for the archive-loading tests.
fetch_rom() {
    local url="$1" zip_sha="$2" zip_name="$3" member="$4" rom_name="$5" rom_sha="$6"
    local rom_path="$roms_dir/$rom_name" zip_path="$roms_dir/$zip_name"
    if [ -f "$zip_path" ] && [ "$(sha_of "$zip_path")" = "$zip_sha" ]; then
        echo "ROM zip ok: $zip_path"
    else
        echo "Downloading $zip_name ..."
        part_files+=("$zip_path.part")
        download_zip "$url" "$zip_path.part"
        if [ "$(sha_of "$zip_path.part")" != "$zip_sha" ]; then
            rm -f "$zip_path.part"
            echo "error: SHA256 mismatch for $zip_name" >&2
            exit 1
        fi
        mv -f "$zip_path.part" "$zip_path"
        echo "ROM zip installed: $zip_path"
    fi
    if [ -f "$rom_path" ] && [ "$(sha_of "$rom_path")" = "$rom_sha" ]; then
        echo "ROM ok: $rom_path"
    else
        part_files+=("$rom_path.part")
        extract_one "$zip_path" "$member" "$rom_path.part"
        if [ "$(sha_of "$rom_path.part")" != "$rom_sha" ]; then
            rm -f "$rom_path.part"
            echo "error: SHA256 mismatch for $rom_name" >&2
            exit 1
        fi
        mv -f "$rom_path.part" "$rom_path"
        echo "ROM installed: $rom_path"
    fi
}

# fetch_core CORE_NAME (TOFU)
fetch_core() {
    local name="$1" core_path="$cores_dir/$1" work sum expected
    local hash_path="$core_path.sha256"
    if [ "$refresh_cores" = 0 ] && [ -f "$core_path" ] && [ -f "$hash_path" ]; then
        expected="$(cut -d' ' -f1 "$hash_path")"
        [ "$(sha_of "$core_path")" = "$expected" ] \
            || { echo "error: $name does not match $hash_path (use --refresh-cores to replace)" >&2; exit 1; }
        echo "Core ok: $core_path ($expected)"
    else
        echo "Downloading $name ..."
        work="$(mktemp -d)"; tmp_dirs+=("$work")
        download_zip "$CORE_BASE_URL/$name.zip" "$work/core.zip"
        extract_one "$work/core.zip" "$name" "$work/$name"
        # Copy next to the destination so the final mv is an atomic same-filesystem rename.
        part_files+=("$core_path.part")
        cp -f "$work/$name" "$core_path.part"
        mv -f "$core_path.part" "$core_path"
        sum="$(sha_of "$core_path")"
        echo "$sum  $name" > "$hash_path"
        echo "Core installed: $core_path (sha256 $sum, recorded as trusted on first use)"
    fi
}

fetch_rom "$ROM_URL" "$ROM_ZIP_SHA256" "$ROM_ZIP_NAME" "$ROM_NAME" "$ROM_NAME" "$ROM_SHA256"
fetch_rom "$SNES_ZIP_URL" "$SNES_ZIP_SHA256" "$SNES_ZIP_NAME" "$SNES_MEMBER" "$SNES_NAME" "$SNES_SHA256"
fetch_rom "$SR_ZIP_URL" "$SR_ZIP_SHA256" "$SR_ZIP_NAME" "$SR_MEMBER" "$SR_NAME" "$SR_SHA256"
fetch_rom "$GG_ZIP_URL" "$GG_ZIP_SHA256" "$GG_ZIP_NAME" "$GG_MEMBER" "$GG_NAME" "$GG_SHA256"
for core in "${CORES[@]}"; do fetch_core "$core"; done
