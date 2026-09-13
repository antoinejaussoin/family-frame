#!/bin/sh
# Convert an RP2350 ELF to UF2 (family rp2350-arm-s) and copy it onto the Pico.
set -e

UF2_ONLY=0
if [ "${1:-}" = "--uf2-only" ]; then
	UF2_ONLY=1
	shift
fi

ELF="${1:?usage: flash-pico.sh [--uf2-only] <firmware.elf>}"
HERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
UF2="$HERE/family-frame.uf2"

python3 "$HERE/elf2uf2_rp2350.py" "$ELF" "$UF2"
echo "UF2: $UF2"

if [ "$UF2_ONLY" -eq 1 ]; then
	exit 0
fi

boot=""
if [ -d /Volumes/RP2350 ]; then
	boot="/Volumes/RP2350"
fi

if [ -z "$boot" ]; then
	cat >&2 <<'EOF'
No RP2350 drive.

Hold BOOTSEL, plug in USB, release, and wait until Finder shows RP2350.
Then run make again. Use a data cable, not a charge-only cable.
EOF
	exit 1
fi

dest="$boot/family-frame.uf2"
echo "Copying onto $boot ..."
python3 - "$UF2" "$dest" "$boot" <<'PY'
import os, shutil, sys, threading

src, dest, boot = sys.argv[1], sys.argv[2], sys.argv[3]
err = {}

def copy():
    try:
        shutil.copyfile(src, dest)
        err["ok"] = True
    except OSError as e:
        # The Pico ejects itself while the copy finishes; that is success.
        err["os"] = e
    except Exception as e:
        err["other"] = e

t = threading.Thread(target=copy, daemon=True)
t.start()
t.join(timeout=15)

if not os.path.isdir(boot):
    print("The RP2350 drive disappeared; that means the Pico accepted the file.")
    sys.exit(0)
if err.get("ok"):
    print(f"Copied to {dest}.")
    sys.exit(0)

print(
    "Could not finish copying onto the Pico automatically.\n"
    f"In Finder, drag\n  {src}\n"
    "onto the RP2350 drive. The drive should vanish after a second.",
    file=sys.stderr,
)
sys.exit(1)
PY
