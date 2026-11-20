#!/usr/bin/env bash
# Fetch the test assets used by the ignored integration tests (ADR 0007):
#   - 240p Test Suite (Mega Drive) 1.21, GPL-2.0 homebrew, SHA256 pinned
#   - 240p Test Suite (SNES) 1.02, GPL-2.0 homebrew (ArtemioUrbina/240pTestSuite), SHA256 pinned
#   - Sub Rescue 0.3 (haroldo-ok, MIT), Master System homebrew, SHA256 pinned
#   - Waternet 1.0 (joyrider3774, MIT), native Game Gear homebrew, SHA256 pinned
#   - gb240p (Game Boy / Color) and 240pee (NES) from 240p-test-mini v0.23 (GPL-2.0, Damian Yerrick/Artemio Urbina), SHA256 pinned
#   - cgb-acid2 1.1 (Matt Currie, MIT), Game Boy Color rendering test, SHA256 pinned
#   - genesis_plus_gx, snes9x and Game Boy / NES candidate libretro cores (gambatte, sameboy, mgba, mesen, nestopia,
#     fceumm), libretro nightly, hash recorded on first download (TOFU)
#   - hatari (Atari ST) libretro core, libretro nightly, TOFU
#   - EmuTOS 1.4 (GPL-2.0-or-later, free TOS): archive emutos-192k-1.4.zip, etos192us.img and etos192fr.img, SHA256 pinned
#     (installed in tests/roms/system/, with tos.img = the US image, the name the hatari core expects)
#   - Nova Defense (geekychris/atari_st_games, MIT), SHA256 pinned, turned into a 720 KB floppy by tests/tools/mkst.py
#     (reproducible output, SHA256 pinned)
#   - 240p Test Suite (Sega CD / Mega CD) 1.21, GPL-2.0 (ArtemioUrbina/240pTestSuite), SHA256 pinned; only the USA
#     disc (cue + iso + 2 wav, ~40 MiB) is extracted. The ISO boot sector carries the standard Sega licence text
#     (format template, not Sega code). Test data only, never shipped in a package.
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/fetch-test-assets.sh [--refresh-cores] [--help]

Downloads test assets into tests/roms and tests/cores (both ignored by git).
Idempotent: files already present and matching their hash are not downloaded again.

  --refresh-cores  re-download the cores and record its new SHA256 (TOFU)
  --help           show this help

Requires: curl, unzip, sha256sum, python3 (floppy image of the Atari ST test program).
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

for tool in curl unzip sha256sum python3; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
roms_dir="$root/tests/roms"
cores_dir="$root/tests/cores"
mkdir -p "$roms_dir" "$cores_dir" "$roms_dir/system"

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

# 240p Test Suite mini (https://github.com/pinobatch/240p-test-mini, LICENSE = GPL-2.0), bare ROMs in the release
MINI_URL='https://github.com/pinobatch/240p-test-mini/releases/download/v0.23'
GB_URL="$MINI_URL/gb240p.gb"
GB_NAME='gb240p-0.23.gb'
GB_SHA256='733eb6e7ec0719752ddd5fcda4e715c2f2e20f4ac4bc971c4bd1d49272f9bb04'
NES_URL="$MINI_URL/240pee.nes"
NES_NAME='240pee-0.23.nes'
NES_SHA256='04f01d7372f66ea2befe325b9bd655a9fcc395a31fa46d9466286bb8f9d2e62e'

# cgb-acid2: https://github.com/mattcurrie/cgb-acid2 (LICENSE = MIT)
CGB_URL='https://github.com/mattcurrie/cgb-acid2/releases/download/v1.1/cgb-acid2.gbc'
CGB_NAME='cgb-acid2-1.1.gbc'
CGB_SHA256='197fb0bcec544f0400527fc707e0a94f55435974986e6986b424ace5de81720e'

CORE_BASE_URL='https://buildbot.libretro.com/nightly/linux/x86_64/latest'
CORES=(
    genesis_plus_gx_libretro.so snes9x_libretro.so
    gambatte_libretro.so sameboy_libretro.so mgba_libretro.so
    mesen_libretro.so nestopia_libretro.so fceumm_libretro.so
    hatari_libretro.so
)

# EmuTOS 1.4 (https://emutos.sourceforge.io/, GPL-2.0-or-later); 192 KB images for ST/STE.
system_dir="$roms_dir/system"
ETOS_ZIP_URL='https://downloads.sourceforge.net/project/emutos/emutos/1.4/emutos-192k-1.4.zip'
ETOS_ZIP_SHA256='59abac06a2d29b0864c5a7cfb2af65f022c337aed34188e174a9a08cc737e4bc'
ETOS_ZIP_NAME='emutos-192k-1.4.zip'
ETOS_US_SHA256='8fbbf8b44fc3e34281eaf8cda5265510e9af9ccda0e3e409111648060d244cfc'
ETOS_FR_SHA256='9fe2fc9c4458818bd0d401fc7a5ad5d487c83a5d5501d75f7737fbb84b9a7692'

# Nova Defense (https://github.com/geekychris/atari_st_games, LICENSE = MIT); the tag is date-based and movable,
# the pinned SHA256 is what protects us.
NOVA_ZIP_URL='https://github.com/geekychris/atari_st_games/releases/download/games-2026-10-30/nova_defense-atari.zip'
NOVA_ZIP_SHA256='e7bce16099f3ae508deb35b386bf9cba4c73f96379f2a14f8eb0b2366cb80c4b'
NOVA_ZIP_NAME='nova_defense-atari-2026-10-30.zip'
NOVA_PRG_SHA256='94a24e206f435522c9e731866841a01a407abfe64c76a16d971f9fb6453f01f5'
NOVA_ST_NAME='nova-defense.st'
NOVA_ST_SHA256='bb3bb0c633094f0df0545017b2e6993d2a15df1f3def774be1614d2e60d7e681'

# 240p Test Suite Sega CD 1.21 (GPL-2.0); the zip holds 3 ISOs (107 MB), only the USA files are extracted.
SCD_ZIP_URL='https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-SegaCD-1.21.zip/download'
SCD_ZIP_SHA256='61e7a67d08cddfff9d8f6b5a901daec87d004a80d28cd1854b104901abe4bde3'
SCD_ZIP_NAME='240pSuite-SegaCD-1.21.zip'
SCD_DIR='240pSuite-SegaCD-1.21-USA'
SCD_MEMBERS=(240p_SegaCD_USA.iso 240pSuite_USA.cue 240p_SegaCD_Track-02.wav 240p_SegaCD_Track-03.wav)
SCD_SHA256S=(
    f0b2a2849b269bdbd1d7f2092197109ff186b96f9617bf55595bb23105ae4428
    6e832cc64eee9782da21562f6869a5b67c482aecbb0dc9311de7baa8ee6fda73
    9d6003f937bb4d7105a7d2e740c3b306efe18e3dbb6db11157fab1528fba418d
    2d819faa2f65f6792c212a79bf846a135430228a57e2d7cf76bffdb915e2042e
)
# Announced uncompressed size of the 4 USA members (bounded extraction: the others are never written to disk).
SCD_MAX_BYTES=$((48 * 1024 * 1024))

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

# fetch_file URL NAME SHA256 : a bare ROM published as is (the archive tests build their own zip).
fetch_file() {
    local url="$1" name="$2" sha="$3" path="$roms_dir/$2"
    if [ -f "$path" ] && [ "$(sha_of "$path")" = "$sha" ]; then
        echo "ROM ok: $path"
        return
    fi
    echo "Downloading $name ..."
    part_files+=("$path.part")
    download_zip "$url" "$path.part"
    if [ "$(sha_of "$path.part")" != "$sha" ]; then
        rm -f "$path.part"
        echo "error: SHA256 mismatch for $name" >&2
        exit 1
    fi
    mv -f "$path.part" "$path"
    echo "ROM installed: $path"
}

# fetch_zip URL ZIP_SHA256 ZIP_PATH : download an archive unless the kept copy already matches.
fetch_zip() {
    local url="$1" sha="$2" path="$3"
    if [ -f "$path" ] && [ "$(sha_of "$path")" = "$sha" ]; then
        echo "Zip ok: $path"
        return
    fi
    echo "Downloading $(basename "$path") ..."
    part_files+=("$path.part")
    download_zip "$url" "$path.part"
    if [ "$(sha_of "$path.part")" != "$sha" ]; then
        rm -f "$path.part"
        echo "error: SHA256 mismatch for $(basename "$path")" >&2
        exit 1
    fi
    mv -f "$path.part" "$path"
}

# install_member ZIP MEMBER DEST SHA256 : extract one member, verify it, install atomically (skipped if already ok).
install_member() {
    local zip="$1" member="$2" dest="$3" sha="$4"
    if [ -f "$dest" ] && [ "$(sha_of "$dest")" = "$sha" ]; then
        echo "File ok: $dest"
        return
    fi
    part_files+=("$dest.part")
    extract_one "$zip" "$member" "$dest.part"
    if [ "$(sha_of "$dest.part")" != "$sha" ]; then
        rm -f "$dest.part"
        echo "error: SHA256 mismatch for $member" >&2
        exit 1
    fi
    mv -f "$dest.part" "$dest"
    echo "File installed: $dest"
}

# fetch_emutos : EmuTOS images plus tos.img (copy of the US image).
fetch_emutos() {
    local zip="$system_dir/$ETOS_ZIP_NAME"
    fetch_zip "$ETOS_ZIP_URL" "$ETOS_ZIP_SHA256" "$zip"
    install_member "$zip" etos192us.img "$system_dir/etos192us.img" "$ETOS_US_SHA256"
    install_member "$zip" etos192fr.img "$system_dir/etos192fr.img" "$ETOS_FR_SHA256"
    if [ ! -f "$system_dir/tos.img" ] || [ "$(sha_of "$system_dir/tos.img")" != "$ETOS_US_SHA256" ]; then
        part_files+=("$system_dir/tos.img.part")
        cp -f "$system_dir/etos192us.img" "$system_dir/tos.img.part"
        mv -f "$system_dir/tos.img.part" "$system_dir/tos.img"
        echo "File installed: $system_dir/tos.img (EmuTOS US)"
    fi
}

# fetch_nova : Nova Defense zip, then the floppy generated by mkst.py (output hash pinned: reproducibility check).
fetch_nova() {
    local zip="$roms_dir/$NOVA_ZIP_NAME" st="$roms_dir/$NOVA_ST_NAME" work
    fetch_zip "$NOVA_ZIP_URL" "$NOVA_ZIP_SHA256" "$zip"
    if [ -f "$st" ] && [ "$(sha_of "$st")" = "$NOVA_ST_SHA256" ]; then
        echo "ROM ok: $st"
        return
    fi
    work="$(mktemp -d)"; tmp_dirs+=("$work")
    extract_one "$zip" NOVA.PRG "$work/NOVA.PRG"
    [ "$(sha_of "$work/NOVA.PRG")" = "$NOVA_PRG_SHA256" ] \
        || { echo "error: SHA256 mismatch for NOVA.PRG" >&2; exit 1; }
    # -I: isolated mode (the script is ours, but never import from the current directory).
    python3 -I "$root/tests/tools/mkst.py" "$work/nova.st" "$work/NOVA.PRG"
    [ "$(sha_of "$work/nova.st")" = "$NOVA_ST_SHA256" ] \
        || { echo "error: generated floppy does not match the pinned SHA256 (mkst.py changed?)" >&2; exit 1; }
    part_files+=("$st.part")
    cp -f "$work/nova.st" "$st.part"
    mv -f "$st.part" "$st"
    echo "ROM installed: $st"
}

# fetch_segacd : 240p Test Suite Sega CD, USA disc only, into tests/roms/$SCD_DIR/ (the cue references the files by name).
fetch_segacd() {
    local zip="$roms_dir/$SCD_ZIP_NAME" dir="$roms_dir/$SCD_DIR" i member dest work todo=0
    for i in "${!SCD_MEMBERS[@]}"; do
        dest="$dir/${SCD_MEMBERS[$i]}"
        if [ -f "$dest" ] && [ "$(sha_of "$dest")" = "${SCD_SHA256S[$i]}" ]; then
            echo "File ok: $dest"
        else
            todo=1
        fi
    done
    [ "$todo" = 1 ] || return 0
    fetch_zip "$SCD_ZIP_URL" "$SCD_ZIP_SHA256" "$zip"
    mkdir -p "$dir"
    work="$(mktemp -d)"; tmp_dirs+=("$work")
    # Exact member names (no wildcard), then a size check on what the archive announces for them.
    local total=0 size
    for member in "${SCD_MEMBERS[@]}"; do
        size="$(unzip -Z -l "$zip" "$member" | awk -v m="$member" '$NF==m {print $4}' | head -n 1)"
        case "$size" in ''|*[!0-9]*) echo "error: $member not found in archive" >&2; exit 1 ;; esac
        total=$((total + size))
    done
    [ "$total" -le "$SCD_MAX_BYTES" ] \
        || { echo "error: Sega CD members too large ($total bytes)" >&2; exit 1; }
    unzip -q -o -j "$zip" "${SCD_MEMBERS[@]}" -d "$work"
    for i in "${!SCD_MEMBERS[@]}"; do
        member="${SCD_MEMBERS[$i]}"
        [ "$(sha_of "$work/$member")" = "${SCD_SHA256S[$i]}" ] \
            || { echo "error: SHA256 mismatch for $member" >&2; exit 1; }
    done
    for member in "${SCD_MEMBERS[@]}"; do
        part_files+=("$dir/$member.part")
        cp -f "$work/$member" "$dir/$member.part"
        mv -f "$dir/$member.part" "$dir/$member"
        echo "File installed: $dir/$member"
    done
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
fetch_file "$GB_URL" "$GB_NAME" "$GB_SHA256"
fetch_file "$NES_URL" "$NES_NAME" "$NES_SHA256"
fetch_file "$CGB_URL" "$CGB_NAME" "$CGB_SHA256"
fetch_emutos
fetch_nova
fetch_segacd
for core in "${CORES[@]}"; do fetch_core "$core"; done
