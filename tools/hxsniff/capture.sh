#!/bin/bash
# capture.sh - drive HX Edit under the USB interposer and leave a capture whose
# traffic is already labelled with the action that caused it.
#
#     bash tools/hxsniff/capture.sh backup     # a full read  (opcode 109)
#     bash tools/hxsniff/capture.sh restore    # a full write (op 109's inverse)
#     bash tools/hxsniff/capture.sh ir         # IR import, export, copy, clear
#     bash tools/hxsniff/capture.sh globals    # every device setting HX Edit exposes
#
# It builds the libusb interposer, launches an instrumented copy of HX Edit
# (logging every USB transfer), walks you through the scenario one step at a
# time, and leaves the capture at  captures/mac-<scenario>-capture.log.
#
# Each step writes a `### MARK` line into the log *before* you act, so
# attribute.py can hand back "these messages are what that click did":
#
#     tools/hxsniff/attribute.py captures/mac-ir-capture.log --marks
#     tools/hxsniff/attribute.py captures/mac-globals-capture.log --summary
#
# That labelling is the whole point of the step-at-a-time pacing. A capture of
# someone clicking freely is a haystack; this one arrives pre-sorted.
#
# DRY=1 walks a scenario's steps without hardware or HX Edit, to read it through
# first:  DRY=1 bash tools/hxsniff/capture.sh ir
#
# run.sh works on a re-signed *copy* of HX Edit; the installed app is untouched.
# Nothing here writes to the pedal beyond what you are asked to click, and each
# scenario ends by putting back what it changed.
set -euo pipefail

SCENARIO="${1:-backup}"
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
WORK="${WORK:-$HOME/.cache/hxsniff}"
LOG="${HXSNIFF_LOG:-$WORK/hxsniff.log}"
MARK="${HXSNIFF_MARK:-$WORK/hxsniff.mark}"
IRS="$WORK/irs"
EXPORTS="$WORK/ir-exports"
OUT="$REPO/captures/mac-${SCENARIO}-capture.log"

# How long to let a step's traffic finish arriving before the next mark is
# written, so an async reply is attributed to the click that caused it.
SETTLE="${SETTLE:-1.0}"

# Prompts read from the terminal rather than stdin, so the script still works
# when it is piped into bash - falling back to stdin where there is no tty.
if (: </dev/tty) 2>/dev/null; then
    ASK=/dev/tty
else
    ASK=/dev/stdin
fi

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
dim() { printf '\033[2m%s\033[0m\n' "$*"; }
rule() { dim "----------------------------------------------------------------"; }

# --- the marking machinery ---------------------------------------------------
# hxsniff.c polls $HXSNIFF_MARK on every transfer and copies its lines into the
# log, so a mark lands at the next piece of traffic - which is why we write it
# before you act rather than after.
mark() { printf '%s\n' "$*" >>"$MARK"; }

# step <label> <instruction...> - mark the log, show the instruction, wait.
step() {
    local label="$1" ans=""
    shift
    sleep "$SETTLE"
    mark "$label"
    printf '\n  \033[1m%s\033[0m\n' "$label"
    printf '      %s\n' "$@"
    read -r -p "      [Enter] when done, or 's' to skip: " ans <"$ASK" || true
    [ "$ans" = "s" ] && mark "$label/SKIPPED"
    return 0
}

# freeform <prefix> - for panels whose controls depend on the connected device,
# which we cannot know from here. You name each control, we mark and wait.
freeform() {
    local prefix="$1" label
    printf '\n      \033[2mName each control as you change it (empty line ends this group).\033[0m\n'
    while :; do
        read -r -p "      ${prefix}/" label <"$ASK" || break
        [ -z "$label" ] && break
        sleep "$SETTLE"
        mark "${prefix}/${label}"
        read -r -p "      change it now, [Enter] when done: " _ <"$ASK" || break
    done
}

case "$SCENARIO" in
backup | restore | ir | globals) ;;
*)
    echo "!! unknown scenario '$SCENARIO' (backup, restore, ir, globals)" >&2
    exit 2
    ;;
esac

# DRY=1 walks the step list without hardware, HX Edit, or a capture - the way to
# read a scenario through before committing the pedal to it.
DRY="${DRY:-0}"

if [ "$DRY" != 1 ]; then
    # The pedal has to be on interface 0 for any of this to mean anything. Only
    # a warning: the check is a guess at how ioreg names it, and being wrong
    # about that should not stop a capture.
    if ! ioreg -p IOUSB -l -w 0 2>/dev/null | grep -qE '"idVendor" = 3649|Line 6'; then
        echo "!! no Line 6 device visible on USB - plug the pedal in and power it up"
        read -r -p "   [Enter] to carry on anyway, Ctrl-C to stop: " _ <"$ASK" || true
    fi

    if [ "$SCENARIO" = ir ] || [ "$SCENARIO" = globals ]; then
        bold "==> writing test impulse responses to $IRS"
        python3 "$HERE/make-irs.py" "$IRS"
        mkdir -p "$EXPORTS"
    fi

    bash "$HERE/run.sh"
    sleep 3
else
    SETTLE=0
fi

rule
bold "  Capture running. Do one step, press Enter, do the next."
bold "  The pedal writes to flash between steps - let it settle before Enter."
rule

case "$SCENARIO" in

# -----------------------------------------------------------------------------
backup)
    step "backup/connect" "Wait for HX Edit to finish connecting to the pedal."
    step "backup/create" "File -> Create Backup...  and save the .hxb somewhere you can find it." \
        "(Keep it: the restore scenario replays this same data back.)"
    step "backup/irs-tab" "Click the IRs tab in the left panel."
    step "backup/prefs" "Open HX Edit -> Preferences (Cmd-,), then close it."
    ;;

# -----------------------------------------------------------------------------
restore)
    step "restore/connect" "Wait for HX Edit to finish connecting to the pedal."
    step "restore/pick-file" "File -> Restore from Backup...  and select the .hxb you made" \
        "with the backup scenario. Do not press Restore yet."
    step "restore/run" "Press Restore Backup and let it finish ('transferring data'" \
        "on the pedal). Same data going back, so nothing is lost."
    ;;

# -----------------------------------------------------------------------------
# The IR path, both directions. The pedal starts with no IRs, so the download
# has to be given something to download first - hence import, then export.
ir)
    printf '\n  Test IRs to import from: \033[1m%s\033[0m\n' "$IRS"
    printf '  Export them back into:   \033[1m%s\033[0m\n' "$EXPORTS"

    step "ir/connect" "Wait for HX Edit to finish connecting to the pedal."
    step "ir/open-tab" "Click the IRs tab in the left panel (this reads the - empty - list)."

    step "ir/import-1-ramp1024" "Select IR slot 1, press IMPORT, choose $IRS/ramp1024.wav" \
        "(1024 samples, s[i] = i/4096, so a sample word says its own index)."
    step "ir/import-2-steps2048" "Slot 2, IMPORT, $IRS/steps2048.wav" \
        "(2048 samples: the maximum, and a staircase of powers of two)."
    step "ir/import-3-stereo512" "Slot 3, IMPORT, $IRS/stereo512.wav" \
        "(stereo; left is +ramp and right is -ramp, so the sign on the wire" \
        " tells us what the Stereo IR Import preference actually did)."
    step "ir/import-4-long96k" "Slot 4, IMPORT, $IRS/long96k.wav" \
        "(96 kHz and 4096 samples - over the rate and over the 2048 limit, so" \
        " this shows what HX Edit converts before it uploads)."

    # The same stereo file under all three import settings: the left channel is
    # +ramp and the right is -ramp, so the sign of what arrives says which
    # channel was taken, and a mix of the two is identically zero. One import
    # per setting is all it takes to decode the preference outright.
    step "ir/stereo-pref-right" "Preferences -> Presets/IRs -> Stereo IR Import: Use Right Channel." \
        "Close Preferences."
    step "ir/import-5-stereo-right" "Slot 5, IMPORT, $IRS/stereo512.wav"
    step "ir/stereo-pref-mix" "Preferences -> Presets/IRs -> Stereo IR Import: Mix Both Channels." \
        "Close Preferences."
    step "ir/import-6-stereo-mix" "Slot 6, IMPORT, $IRS/stereo512.wav"
    step "ir/stereo-pref-left" "Preferences -> Presets/IRs -> Stereo IR Import: back to" \
        "Use Left Channel. Close Preferences."

    step "ir/rename-2" "Right-click slot 2 -> Rename -> type  steps-renamed"
    step "ir/select-1" "Click slot 1 once, nothing else." \
        "(A baseline: what mere selection reads, as opposed to a transfer.)"

    step "ir/export-1" "Slot 1 selected, press EXPORT, save into $EXPORTS" \
        "*** this is the download we are here for ***"
    step "ir/export-2" "Slot 2, EXPORT, into the same folder."
    step "ir/export-4" "Slot 4, EXPORT - the one that was resampled on the way in," \
        "so the file that comes back says what the pedal actually stored."
    step "ir/export-5-6-stereo" "Slots 5 and 6, EXPORT each into that folder." \
        "(Signs and zeros in these two decode the Stereo IR Import preference.)"
    step "ir/export-multi" "Select slots 1-4 together (shift-click), EXPORT to that folder." \
        "(Whether a batch export is N single reads or one bulk read.)"

    step "ir/copy-1-paste-8" "Select slot 1, COPY, select slot 8, PASTE." \
        "(A copy inside the device: does it move samples over USB at all?)"
    step "ir/drag-8-to-9" "Drag slot 8 onto slot 9. Skip with 's' if it will not drag."
    step "ir/use-in-preset" "In the editor, put an IR block in the chain and point it at IR 1." \
        "Do NOT save the preset - we want the pedal left as we found it."

    step "ir/backup-with-irs" "File -> Create Backup... , save as irs-present.hxb" \
        "(A backup taken with IRs on board: it shows where IRs sit in the" \
        " opcode-109 object store and in the .hxb blocks. Keep this file.)"

    step "ir/clear-9" "Select slot 9, press CLEAR."
    step "ir/clear-rest" "Select slots 1-6 and 8, press CLEAR - leaving the pedal as found."
    step "ir/reload-preset" "Reselect the preset you were on, discarding the IR block edit."
    step "ir/final-list" "Click the PRESETS tab and back to IRs, confirming the list is empty."
    ;;

# -----------------------------------------------------------------------------
# Every device setting HX Edit can write. Most of the pedal's ~150 globals live
# only in its own Global Settings menu; HX Edit reaches the Preferences dialog,
# the Global EQ window and the tempo field, and that is all. The step list below
# is what an HX Stomp actually shows in 3.82 - Preferences has View,
# Presets/IRs and Device Settings, and neither the Hardware Compatibility panel
# nor the library-clearing buttons appear while a device is connected.
#
# What the first run of this established (captures/mac-globals-capture.log):
# ids 27 preset numbering, 95 EXP/FS Tip, 96 EXP/FS Ring, 97/98/99 FS3/FS4/FS5
# function, 190-200 the Global EQ coefficients, 203 EQ enabled, 16 tempo,
# 14 tempo mode. Name labels, Manage Favorites By and Stereo IR Import send
# nothing at all: they are HX Edit's own preferences, not the pedal's.
globals)
    step "globals/connect" "Wait for HX Edit to finish connecting to the pedal."
    step "globals/open-prefs" "Open HX Edit -> Preferences (Cmd-,). Stop there." \
        "(Opening it reads the Device Settings ids - that read is half the mapping.)"

    step "globals/view-tab" "Preferences -> View tab."
    step "globals/view-names-hide" "Show/hide name labels: pick Hide Names."
    step "globals/view-names-show" "Put it back to Show Names."
    step "globals/view-favs-category" "Manage Favorites By: pick Category."
    step "globals/view-favs-name" "Manage Favorites By: pick Name (A-Z)."
    step "globals/view-favs-device" "Manage Favorites By: back to Device List."

    step "globals/presets-irs-tab" "Preferences -> Presets/IRs tab."
    step "globals/numbering-000" "Preset Numbering Format: pick 000-125."
    step "globals/numbering-01a" "Preset Numbering Format: back to 01A-42C."
    step "globals/stereo-ir-right" "Stereo IR Import: Use Right Channel."
    step "globals/stereo-ir-mix" "Stereo IR Import: Mix Both Channels."
    step "globals/stereo-ir-left" "Stereo IR Import: back to Use Left Channel."
    echo
    dim "      Deliberately not doing: Restore Factory Settings - it wipes the"
    dim "      pedal's globals, and a single setting write shows more anyway."

    # Type each label BEFORE you change the control: the mark has to reach the
    # log ahead of the traffic it explains. Ids disambiguate either way, but the
    # enum values only line up if the labels sit where they belong.
    step "globals/devsettings-tab" "Preferences -> Device Settings tab."
    step "globals/dev-tip-fs4" "EXP/FS Tip: EXP 1 -> FS4."
    step "globals/dev-tip-exp1" "EXP/FS Tip: back to EXP 1."
    step "globals/dev-ring-exp2" "EXP/FS Ring: FS5 -> EXP 2."
    step "globals/dev-ring-fs5" "EXP/FS Ring: back to FS5."
    printf '\n      Now FS3, FS4 and FS5 Function. Step each dropdown through every\n'
    printf '      entry it offers - Tap/Tuner, Stomp N, Preset Up/Down, Snapshot\n'
    printf '      Up/Down, All Bypass, Toggle EXP, and on FS4/FS5 also Bank Up/Down\n'
    printf '      and FS Mode >/< FS Mode - naming each one, so the integer under\n'
    printf '      key 119 can be matched to the function it selects.\n'
    freeform "globals/fs"

    step "globals/dev-restore" "Put every Device Settings dropdown back where it was."

    # Five bands: Low Cut, three peaks of freq/Q/gain, High Cut - the eleven
    # coefficients op76 returns under key 55, in that array's order, which is
    # ids 190..200. Each step asks for a *distinct* value so the write carries a
    # number that appears nowhere else. (globaleq.xml also has an APPLY EQ
    # selector for 1/4" vs XLR outputs; an HX Stomp has no XLRs, so HX Edit
    # hides it. On a Helix Floor/Rack/LT, add it here.)
    step "globals/eq-open" "Open the Global EQ window from the menu." \
        "(It reads ids 201 and 202, and pulls the coefficients with op76.)"
    step "globals/eq-enable" "Click the power button, top right, so the EQ is switched on."

    step "globals/eq-lowcut-freq" "Low Cut -> Freq: from Off to 41 Hz."
    step "globals/eq-lowpeak-freq" "Low Peak -> Freq: 110 -> 122 Hz."
    step "globals/eq-lowpeak-q" "Low Peak -> Q: 0.7 -> 1.1."
    step "globals/eq-lowpeak-gain" "Low Peak -> Gain: 0.0 -> +2.0 dB."
    step "globals/eq-midpeak-freq" "Mid Peak -> Freq: 2.0 -> 1.23 kHz."
    step "globals/eq-midpeak-q" "Mid Peak -> Q: 0.7 -> 2.2."
    step "globals/eq-midpeak-gain" "Mid Peak -> Gain: 0.0 -> -4.0 dB."
    step "globals/eq-highpeak-freq" "High Peak -> Freq: 8.0 -> 4.32 kHz."
    step "globals/eq-highpeak-q" "High Peak -> Q: 0.7 -> 3.3."
    step "globals/eq-highpeak-gain" "High Peak -> Gain: 0.0 -> +6.0 dB." \
        "(id 198 - inferred from op76's array, never yet seen written.)"
    step "globals/eq-highcut-freq" "High Cut -> Freq: from Off to 12.3 kHz." \
        "(id 200 - likewise inferred, so this one is worth doing.)"

    step "globals/eq-drag-curve" "Drag one of the circles on the curve itself, rather" \
        "than typing in a field - whether the graph writes the same way." \
        "Skip with 's' if you would rather not."
    step "globals/eq-reset" "Press RESET." \
        "(op77: resets and returns the same array op76 does.)"
    step "globals/eq-disable" "Switch the global EQ back off with the power button," \
        "and close the window."

    step "globals/tempo-value" "In the editor, set the tempo field to 123.0 BPM." \
        "(id 16.)"
    step "globals/tempo-mode" "Click the tempo readout's menu and step through all four" \
        "of Per Snapshot / Per Preset / Global / Host Sync, ending where you" \
        "started. (id 14 - only one of its four values is known so far.)"
    step "globals/tempo-restore" "Put the tempo back to what it was."

    step "globals/restore-globals-only" "Optional, and worth it: File -> Restore from Backup," \
        "pick a .hxb you made, tick ONLY Global Settings, and restore." \
        "That is the same values going back, and it isolates exactly which" \
        "objects the globals half of a restore writes. Skip with 's'."
    ;;
esac

rule
if [ "$DRY" = 1 ]; then
    bold "  Dry run - nothing was captured."
    exit 0
fi
bold "  Steps done. Quit HX Edit (Cmd-Q) - the capture ends when it exits."
rule

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
bold "Capture saved: $OUT  ($(wc -l <"$OUT") lines)"
echo
echo "  What it caught, by step:"
echo "      tools/hxsniff/attribute.py $OUT --marks"
echo "      tools/hxsniff/attribute.py $OUT --summary"
echo
if [ "$SCENARIO" = ir ]; then
    echo "  Keep the exported IRs next to the capture - the wire bytes are only"
    echo "  decodable against the files they came back as:"
    echo
    echo "      mkdir -p captures/ir-exports && cp $EXPORTS/*.wav captures/ir-exports/"
    echo
fi
echo "  Then commit and push it back:"
echo
echo "      git add captures/mac-${SCENARIO}-capture.log"
echo "      git commit -m 'Capture: HX Edit ${SCENARIO}'"
echo "      git push"
