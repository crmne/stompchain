#!/usr/bin/env python3
"""Write test impulse responses whose samples are readable in a hexdump.

Capturing HX Edit's IR *download* means finding the sample bytes in a reply
stream that has no framing we understand yet. A real cab IR is noise for that
purpose — one f32 word looks like any other. These files are built so that any
four bytes on the wire say where they came from:

  ramp1024    s[i] = i / 4096          exact in f32 and in 24-bit PCM, so the
                                       value *is* the sample index; a lone word
                                       anywhere in the stream gives its offset
  steps2048   a staircase of powers of two, held 128 samples each, so every word
                                       is a clean constant (3f000000, 3e800000,
                                       …) and chunk boundaries jump out
  stereo512   L = +i/4096, R = -i/4096  the sign says which channel survived the
                                       Stereo IR Import preference
  long96k     4096 samples at 96 kHz    over both the length limit (2048) and the
                                       device rate, so the capture shows what
                                       HX Edit converts before it uploads

24-bit PCM because every importer accepts it and i/4096 stays exact (i × 2048).

    ./make-irs.py [DIR]        default ~/.cache/hxsniff/irs
"""

import os
import struct
import sys

FULL = 1 << 23  # 24-bit full scale


def wav(path, rate, channels, frames):
    """Write frames — a list of per-channel int tuples — as a 24-bit PCM WAV."""
    data = bytearray()
    for frame in frames:
        for v in frame:
            data += struct.pack("<i", v)[:3]  # little-endian, drop the top byte

    block = 3 * channels
    hdr = struct.pack(
        "<4sI4s4sIHHIIHH4sI",
        b"RIFF", 36 + len(data), b"WAVE",
        b"fmt ", 16, 1, channels, rate, rate * block, block, 24,
        b"data", len(data),
    )
    with open(path, "wb") as f:
        f.write(hdr + data)
    return path


def main():
    dest = sys.argv[1] if len(sys.argv) > 1 else \
        os.path.expanduser("~/.cache/hxsniff/irs")
    os.makedirs(dest, exist_ok=True)

    # s[i] = i / 4096 → 24-bit value i × 2048, so the word on the wire is the
    # sample index shifted left by 11.
    ramp = [(i * (FULL // 4096),) for i in range(1024)]

    # Sixteen levels, 0.5 down to 2**-16, each held for 128 samples. Peaks below
    # full scale so a normalising importer has room to show itself.
    steps = [((FULL >> 1) >> (i // 128),) for i in range(2048)]

    # Same ramp on the left, negated on the right: the sign identifies the
    # channel, and a mix of the two is identically zero.
    stereo = [(i * (FULL // 4096), -i * (FULL // 4096)) for i in range(512)]

    # Twice the device's rate and twice the samples it can store.
    long96k = [(i * (FULL // 8192),) for i in range(4096)]

    made = [
        wav(os.path.join(dest, "ramp1024.wav"), 48000, 1, ramp),
        wav(os.path.join(dest, "steps2048.wav"), 48000, 1, steps),
        wav(os.path.join(dest, "stereo512.wav"), 48000, 2, stereo),
        wav(os.path.join(dest, "long96k.wav"), 96000, 1, long96k),
    ]
    for p in made:
        print(f"{os.path.getsize(p):>8}  {p}")


if __name__ == "__main__":
    main()
