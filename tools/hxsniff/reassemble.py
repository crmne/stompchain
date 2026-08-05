#!/usr/bin/env python3
"""Reassemble the HX byte streams from a capture and decode them as MessagePack.

Each USB message carries an 8-byte transport header (see decode.py) followed by
a payload that belongs to one of several independent bidirectional channels.
Within a channel the payload begins with its own 8-byte header:

    +0  u16   sequence number
    +2  u16   message type   (bit 0x04 carries data, 0x08 is a bare acknowledgement)
    +4  u32   acknowledgement — bytes received so far from the peer

Everything after that is a byte stream that is chunked arbitrarily across USB
messages, so it has to be concatenated per channel before it can be parsed.

The stream itself turns out to be MessagePack, which is the whole point of this
script: if concatenating is done right, a strict MessagePack reader consumes the
stream cleanly, and if it is done wrong the reader desynchronises immediately.
That makes it a sharp test of the framing rather than a guess.
"""

import argparse
import collections
import struct
import sys

import decode as dec


# ------------------------------------------------------------- messagepack ---
class MsgPack:
    """Minimal strict MessagePack reader that reports where it stopped."""

    def __init__(self, buf):
        self.b, self.i = buf, 0

    def u(self, n, fmt):
        v = struct.unpack_from(fmt, self.b, self.i)[0]
        self.i += n
        return v

    def raw(self, n):
        if self.i + n > len(self.b):
            raise EOFError
        v = self.b[self.i : self.i + n]
        self.i += n
        return v

    def read(self):
        if self.i >= len(self.b):
            raise EOFError
        c = self.b[self.i]
        self.i += 1
        if c <= 0x7F:
            return c
        if c >= 0xE0:
            return c - 0x100
        if 0x80 <= c <= 0x8F:
            return self.map(c & 0x0F)
        if 0x90 <= c <= 0x9F:
            return [self.read() for _ in range(c & 0x0F)]
        if 0xA0 <= c <= 0xBF:
            return self.str(c & 0x1F)
        return self.ext(c)

    def ext(self, c):
        if c == 0xC0:
            return None
        if c == 0xC2:
            return False
        if c == 0xC3:
            return True
        if c == 0xC4:
            return self.raw(self.u(1, "B"))
        if c == 0xC5:
            return self.raw(self.u(2, ">H"))
        if c == 0xC6:
            return self.raw(self.u(4, ">I"))
        if c == 0xCA:
            return round(self.u(4, ">f"), 6)
        if c == 0xCB:
            return self.u(8, ">d")
        if c == 0xCC:
            return self.u(1, "B")
        if c == 0xCD:
            return self.u(2, ">H")
        if c == 0xCE:
            return self.u(4, ">I")
        if c == 0xCF:
            return self.u(8, ">Q")
        if c == 0xD0:
            return self.u(1, "b")
        if c == 0xD1:
            return self.u(2, ">h")
        if c == 0xD2:
            return self.u(4, ">i")
        if c == 0xD3:
            return self.u(8, ">q")
        if c == 0xD9:
            return self.str(self.u(1, "B"))
        if c == 0xDA:
            return self.str(self.u(2, ">H"))
        if c == 0xDB:
            return self.str(self.u(4, ">I"))
        if c == 0xDC:
            return [self.read() for _ in range(self.u(2, ">H"))]
        if c == 0xDD:
            return [self.read() for _ in range(self.u(4, ">I"))]
        if c == 0xDE:
            return self.map(self.u(2, ">H"))
        if c == 0xDF:
            return self.map(self.u(4, ">I"))
        raise ValueError(f"unknown tag {c:#04x} at {self.i-1}")

    def map(self, n):
        return {self._key(self.read()): self.read() for _ in range(n)}

    @staticmethod
    def _key(k):
        return k if isinstance(k, (int, str)) else repr(k)

    def str(self, n):
        s = self.raw(n)
        # Line 6 stores C strings, so the declared length includes the NUL.
        # The same string type is also used for opaque blobs — notably the
        # preset body, which is itself a MessagePack document — so anything
        # that is not clean text is handed back as bytes for the caller to
        # re-parse rather than being mangled into replacement characters.
        body = s.rstrip(b"\0")
        try:
            t = body.decode("ascii")
        except UnicodeDecodeError:
            return Blob(body)
        return t if all(c == "\t" or c >= " " for c in t) else Blob(body)


class Blob(bytes):
    """Opaque payload; may hold a nested MessagePack document."""

    def nested(self):
        """Decode as a sequence of MessagePack values, or None if it isn't one."""
        mp = MsgPack(self)
        out = []
        try:
            while mp.i < len(self):
                out.append(mp.read())
        except Exception:
            return None
        return out


# ------------------------------------------------------------- reassembly ---
def channels(msgs):
    """Group payloads into per-direction streams keyed by (src, dst)."""
    out = collections.defaultdict(list)
    for m in msgs:
        if not m.ok or len(m.payload) < 8:
            continue
        p = m.payload
        seq, typ = struct.unpack_from(">HH", p, 0)
        # Bit 0x04 means "carries stream data". It is set on plain data (0x04),
        # on data that piggybacks an acknowledgement (0x0c), and on data that
        # rides a keep-alive slot (0x14). Matching only 0x04 drops real messages.
        if not (typ & 0x04) or len(p) <= 8:
            continue  # 0x02 handshake, 0x08 pure ack, 0x10 keep-alive
        out[(m.src, m.dst)].append((seq, m.seq, p[8:]))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--skip", type=int, default=8,
                    help="bytes of per-message header to skip on a stream start")
    ap.add_argument("--list", action="store_true", help="list channels only")
    ap.add_argument("--chan", help="channel as SRC:DST hex, e.g. 1080:03ed")
    ap.add_argument("--max", type=int, default=40, help="max objects to print")
    ap.add_argument("--depth", type=int, default=2, help="nesting depth to expand")
    args = ap.parse_args()

    msgs = [m for m in dec.parse(args.log) if dec.interesting(m)]
    chans = channels(msgs)

    if args.list or not args.chan:
        print("channel (src -> dst)   chunks   bytes")
        for (s, d), items in sorted(chans.items()):
            print(f"  {s:#06x} -> {d:#06x}   {len(items):6}  "
                  f"{sum(len(x[2]) for x in items):8}")
        if not args.chan:
            return

    s, d = (int(x, 16) for x in args.chan.split(":"))
    stream = b"".join(c for _, _, c in chans[(s, d)])
    print(f"stream {s:#06x} -> {d:#06x}: {len(stream)} bytes\n")

    for i, (pos, hdr, obj, err) in enumerate(messages(stream)):
        if i >= args.max:
            break
        if err:
            print(f"[{pos:6}] !! {err}")
            break
        print(f"[{pos:6}] hdr={hdr[0]:#06x}/{hdr[1]:#06x} len={hdr[2]:<5} "
              f"{summarise(obj, args.depth)}")


def messages(stream):
    """Walk the length-prefixed messages that make up a channel stream.

    Each is an 8-byte header whose last 4 bytes are a little-endian length,
    followed by exactly that many bytes of MessagePack.
    """
    pos = 0
    while pos + 8 <= len(stream):
        f0, f1, ln = struct.unpack_from("<HHI", stream, pos)
        body = stream[pos + 8 : pos + 8 + ln]
        if len(body) < ln:
            yield pos, (f0, f1, ln), None, f"truncated ({len(body)}/{ln})"
            return
        try:
            obj = MsgPack(body).read()
            err = None
        except Exception as e:
            obj, err = None, f"decode failed: {e}"
        yield pos, (f0, f1, ln), obj, err
        pos += 8 + ln


def summarise(o, depth, d=0):
    """Compact rendering that keeps deep structures readable."""
    pad = "  " * d
    if isinstance(o, Blob):
        inner = o.nested()
        if inner is None:
            return f"<blob {len(o)} bytes: {o[:24].hex(' ')}...>"
        body = ", ".join(summarise(v, depth, d + 1) for v in inner)
        return f"<nested {len(o)}B>[{body}]"
    if isinstance(o, dict):
        if d >= depth:
            return f"{{...{len(o)} keys...}}"
        items = [f"\n{pad}    {k}: {summarise(v, depth, d+1)}" for k, v in o.items()]
        return "{" + ",".join(items) + f"\n{pad}  }}"
    if isinstance(o, list):
        if d >= depth or len(o) > 24:
            return f"[...{len(o)} items...]"
        return "[" + ", ".join(summarise(v, depth, d + 1) for v in o) + "]"
    return repr(o)


if __name__ == "__main__":
    main()
