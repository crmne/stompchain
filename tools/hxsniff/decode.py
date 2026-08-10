#!/usr/bin/env python3
"""Turn an hxsniff log into structured HX protocol messages.

Working framing hypothesis (see PROTOCOL.md):

    offset 0  u32 LE   low 24 bits = payload length, high 8 bits = flags
    offset 4  u16 LE   destination node
    offset 6  u16 LE   source node
    offset 8  payload, padded out to a 4-byte boundary

Nothing here is authoritative; the point is to make patterns visible so the
framing can be confirmed or refuted against a lot of messages at once.
"""

import argparse
import collections
import re
import sys

HDR = re.compile(
    r"^\[(?P<seq>\d+)\] \+(?P<t>[\d.]+) (?P<kind>\S+)(?: ep=(?P<ep>0x[0-9a-f]+))?"
    r"(?: len=(?P<len>\d+))?(?P<rest>.*)$"
)
HEXLINE = re.compile(r"^\t[0-9a-f]{4}  (.*?)\|")
MARK = re.compile(r"^### MARK (.*)$")


class Msg:
    __slots__ = ("seq", "t", "kind", "ep", "data", "extra", "mark")

    def __init__(self, seq, t, kind, ep, extra):
        self.seq, self.t, self.kind, self.ep = seq, t, kind, ep
        self.extra, self.data, self.mark = extra, bytearray(), None

    @property
    def out(self):
        return self.ep is not None and not (self.ep & 0x80)

    # --- framing -----------------------------------------------------------
    @property
    def ok(self):
        return len(self.data) >= 8

    @property
    def plen(self):
        return int.from_bytes(self.data[0:3], "little")

    @property
    def flags(self):
        return self.data[3]

    @property
    def dst(self):
        return int.from_bytes(self.data[4:6], "little")

    @property
    def src(self):
        return int.from_bytes(self.data[6:8], "little")

    @property
    def payload(self):
        return bytes(self.data[8 : 8 + self.plen])

    @property
    def framing_ok(self):
        """Total length should be the header plus the payload rounded up to 4."""
        want = 8 + (self.plen + 3) // 4 * 4
        return want == len(self.data)


def parse(path):
    msgs, cur, pending_mark = [], None, None
    for line in open(path, errors="replace"):
        m = MARK.match(line)
        if m:
            pending_mark = m.group(1).strip()
            continue
        h = HEXLINE.match(line.rstrip("\n"))
        if h and cur is not None:
            cur.data.extend(bytes.fromhex(h.group(1).replace(" ", "")))
            continue
        m = HDR.match(line)
        if m:
            cur = Msg(
                int(m.group("seq")),
                float(m.group("t")),
                m.group("kind"),
                int(m.group("ep"), 16) if m.group("ep") else None,
                m.group("rest").strip(),
            )
            cur.mark, pending_mark = pending_mark, None
            msgs.append(cur)
    return msgs


def interesting(m):
    """Drop the endpoint-poll bookkeeping that carries no protocol content."""
    if m.kind in ("ASYNC-IN-SUBMIT", "ASYNC-OUT-DONE"):
        return False
    if m.kind == "ASYNC-IN-DONE" and not m.data:
        return False
    return True


def fmt(m, args):
    if not m.ok:
        return f"  {m.t:9.3f} {m.kind} {m.extra}"
    arrow = "-->" if m.out else "<--"
    node = f"{m.dst:#06x}{arrow}{m.src:#06x}" if m.out else f"{m.src:#06x}{arrow}{m.dst:#06x}"
    warn = "" if m.framing_ok else "  !!FRAMING"
    p = m.payload
    body = p.hex(" ") if args.full else p[: args.width].hex(" ")
    if not args.full and len(p) > args.width:
        body += f" ...(+{len(p)-args.width})"
    return (
        f"  {m.t:9.3f} [{m.seq:5}] {node} f={m.flags:#04x} n={m.plen:<4} {body}{warn}"
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--full", action="store_true", help="print whole payloads")
    ap.add_argument("--width", type=int, default=40, help="payload bytes shown")
    ap.add_argument("--stats", action="store_true", help="summarise instead")
    ap.add_argument("--since", help="only after this mark substring")
    ap.add_argument("--node", help="only this node id, e.g. 0x1001")
    args = ap.parse_args()

    # A mark lands on whatever line follows it, which is usually one of the
    # poll-bookkeeping entries we drop - so carry it forward to the next
    # message we actually keep, or it would vanish.
    msgs, carried = [], None
    for m in parse(args.log):
        if not interesting(m):
            carried = m.mark or carried
            continue
        if carried:
            m.mark = m.mark or carried
            carried = None
        msgs.append(m)

    if args.since:
        keep, on = [], False
        for m in msgs:
            if m.mark and args.since in m.mark:
                on = True
            if on:
                keep.append(m)
        msgs = keep
    if args.node:
        want = int(args.node, 0)
        msgs = [m for m in msgs if m.ok and want in (m.src, m.dst)]

    if args.stats:
        chans = collections.Counter()
        flags = collections.Counter()
        bad = 0
        for m in msgs:
            if not m.ok:
                continue
            lo, hi = sorted((m.src, m.dst))
            chans[(lo, hi)] += 1
            flags[m.flags] += 1
            bad += not m.framing_ok
        print(f"messages: {len(msgs)}   framing mismatches: {bad}")
        print("\nchannels (node pair -> count):")
        for (a, b), n in chans.most_common():
            print(f"  {a:#06x} <-> {b:#06x}   {n}")
        print("\nflag byte (header byte 3):")
        for f, n in flags.most_common():
            print(f"  {f:#04x}  {n}")
        return

    for m in msgs:
        if m.mark:
            print(f"\n=== {m.mark} ===")
        print(fmt(m, args))


if __name__ == "__main__":
    main()
