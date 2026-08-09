#!/bin/bash
# capture-backup.sh - one-shot capture of HX Edit performing a full backup, so we
# can reverse-engineer how it reads all 126 presets (the "fast read"), plus its
# IR and global-settings reads.
#
# Run this on the Mac with the HX Stomp plugged into the Mac:
#
#     bash tools/hxsniff/capture-backup.sh
#
# It builds the libusb interposer, launches an instrumented copy of HX Edit
# (logging every USB transfer), waits while you do a backup, then leaves the
# capture at  captures/mac-backup-capture.log  ready to commit and push back.
#
# Nothing here touches the installed HX Edit or the pedal's contents: HX Edit
# only *reads* during a backup, and run.sh works on a re-signed copy of the app.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
WORK="${WORK:-$HOME/.cache/hxsniff}"
LOG="${HXSNIFF_LOG:-$WORK/hxsniff.log}"
OUT="$REPO/captures/mac-backup-capture.log"

# Build the interposer and launch the instrumented HX Edit (it logs to $LOG and
# runs in the background; run.sh returns once it is up).
bash "$HERE/run.sh"

cat <<'STEPS'

================================================================
  Capture is running. In the HX Edit window that just opened:

    1. Wait for it to connect to the HX Stomp.
    2. File -> Create Backup...   and save the .hxb somewhere.
    3. Click the "Impulses" tab            (captures IR reads).
    4. Open Preferences / Global Settings  (captures globals).
    5. Quit HX Edit  (Cmd-Q)   <-- the capture ends when it quits.
================================================================

Waiting for HX Edit to quit...
STEPS

# Give it a moment to appear, then wait for the instrumented copy to exit.
sleep 3
while pgrep -f "hxsniff/HX Edit.app/Contents/MacOS/HX Edit" >/dev/null 2>&1; do
    sleep 1
done

if [ ! -s "$LOG" ]; then
    echo "!! No capture at $LOG - did HX Edit launch and connect?" >&2
    exit 1
fi

mkdir -p "$REPO/captures"
cp "$LOG" "$OUT"
echo
echo "Capture saved: $OUT  ($(wc -l <"$OUT") lines)"
echo "Push it back so the Linux side can decode it:"
echo
echo "    git add captures/mac-backup-capture.log"
echo "    git commit -m 'Capture: HX Edit full backup'"
echo "    git push"
