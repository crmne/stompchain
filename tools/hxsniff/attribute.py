#!/usr/bin/env python3
"""Attribute decoded HX application messages to the capture marks that caused them.

`reassemble.py` can turn a channel into a clean MessagePack stream but throws away
the one piece of information that makes a capture interpretable: which `### MARK`
line — that is, which UI click — each message belongs to.

The difficulty is that the application stream is chunked arbitrarily across USB
transfers, so a stream offset has no timestamp of its own. This script keeps an
index alongside the concatenated stream recording, for every byte range, which USB
transfer contributed it. Once a stream message is parsed at offset `pos`, the
contributing transfer is looked up and the mark in force at that transfer is the
message's mark.

Usage:

    ./attribute.py CAPTURE.log                     # every message, grouped by mark
    ./attribute.py CAPTURE.log --mark PARAM-       # only marks matching a substring
    ./attribute.py CAPTURE.log --opcode 30         # only requests/replies for op 30
    ./attribute.py CAPTURE.log --summary           # opcode x mark cross-tab
    ./attribute.py CAPTURE.log --marks             # every mark, empty ones included
    ./attribute.py A.log B.log --dict              # opcode + notification inventory

Channels are named by their device node throughout: 0x1001 device/session control
(host 0x03ef), 0x1002 UI and state notifications (host 0x03f0), 0x1080 preset and
global data (host 0x03ed).
"""

import argparse
import collections
import struct

import decode as dec
from reassemble import Blob, MsgPack


class Rec:
    """One decoded application message with its provenance."""

    __slots__ = ("chan", "out", "pos", "hdr", "obj", "err", "t", "usb", "mark", "for_op")

    def __init__(self, chan, out, pos, hdr, obj, err, t, usb, mark):
        self.chan, self.out, self.pos, self.hdr = chan, out, pos, hdr
        self.obj, self.err, self.t, self.usb, self.mark = obj, err, t, usb, mark
        self.for_op = None

    # --- application-layer shape -------------------------------------------
    @property
    def kind(self):
        o = self.obj
        if not isinstance(o, dict):
            return "?"
        if 100 in o:
            return "req"
        if 103 in o or (102 in o and 104 in o):
            return "rsp"
        if 105 in o:
            return "note"
        return "?"

    @property
    def opcode(self):
        return self.obj.get(100) if isinstance(self.obj, dict) else None

    @property
    def txn(self):
        return self.obj.get(102) if isinstance(self.obj, dict) else None

    @property
    def event(self):
        return self.obj.get(105) if isinstance(self.obj, dict) else None

    @property
    def args(self):
        if not isinstance(self.obj, dict):
            return None
        for k in (101, 104, 106):
            if k in self.obj:
                return self.obj[k]
        return None


def load(path):
    """Parse a capture and carry each mark forward to the next kept transfer."""
    msgs, carried = [], None
    for m in dec.parse(path):
        if not dec.interesting(m):
            carried = m.mark or carried
            continue
        if carried:
            m.mark = m.mark or carried
            carried = None
        msgs.append(m)
    return msgs


def mark_timeline(msgs):
    """Ordered (transfer position, mark) pairs, so a mark can be looked up by index."""
    return [(i, m.mark) for i, m in enumerate(msgs) if m.mark]


def mark_at(timeline, idx):
    """The mark in force at transfer `idx` — the last one attached at or before it."""
    cur = None
    for i, name in timeline:
        if i > idx:
            break
        cur = name
    return cur


def streams(msgs):
    """Per-direction concatenated streams plus a byte-offset -> transfer index map.

    Returns {(src, dst): (bytes, [(start_offset, transfer_index, timestamp), ...])}.
    """
    buf = collections.defaultdict(bytearray)
    idx = collections.defaultdict(list)
    for i, m in enumerate(msgs):
        if not m.ok or len(m.payload) < 8:
            continue
        p = m.payload
        _seq, typ = struct.unpack_from(">HH", p, 0)
        # Bit 0x04 is "carries stream data". It is set on plain data (0x04) but
        # also on data that piggybacks an acknowledgement (0x0c) or rides a
        # keep-alive slot (0x14). Reading only 0x04 silently drops messages —
        # in 03-feature-sweep that hid two thirds of every slider drag.
        if not (typ & 0x04) or len(p) <= 8:
            continue  # 0x02 handshake, 0x08 bare ack, 0x10 keep-alive
        key = (m.src, m.dst)
        idx[key].append((len(buf[key]), i, m.t))
        buf[key].extend(p[8:])
    return {k: (bytes(v), idx[k]) for k, v in buf.items()}


def locate(index, pos):
    """Which transfer contributed the byte at stream offset `pos`."""
    lo, hi, best = 0, len(index) - 1, index[0] if index else (0, 0, 0.0)
    while lo <= hi:
        mid = (lo + hi) // 2
        if index[mid][0] <= pos:
            best, lo = index[mid], mid + 1
        else:
            hi = mid - 1
    return best


def walk(stream):
    """Walk the 8-byte-prefixed application messages of a channel stream."""
    pos = 0
    while pos + 8 <= len(stream):
        f0, f1, ln = struct.unpack_from("<HHI", stream, pos)
        body = stream[pos + 8 : pos + 8 + ln]
        if len(body) < ln:
            yield pos, (f0, f1, ln), None, f"truncated ({len(body)}/{ln})"
            return
        try:
            obj, err = MsgPack(body).read(), None
        except Exception as e:  # noqa: BLE001 - want the reason in the output
            obj, err = None, f"decode failed: {e}"
        yield pos, (f0, f1, ln), obj, err
        pos += 8 + ln


def raw_activity(path):
    """Per-mark counts of *transfers*, so an empty mark can be told from a missed decode.

    A mark with zero data-carrying transfers means the click genuinely produced no
    USB traffic; a mark with transfers but no decoded messages would mean the
    decoder is at fault. Keep-alives (type 0x10) and bare acks (0x08) are counted
    separately because they happen once a second regardless of what the user does.
    """
    msgs = load(path)
    timeline = mark_timeline(msgs)
    tab = collections.defaultdict(collections.Counter)
    order, seen = [], set()
    for _, name in timeline:
        if name not in seen:
            order.append(name)
            seen.add(name)
    for i, m in enumerate(msgs):
        if not m.ok or len(m.payload) < 8:
            continue
        _seq, typ = struct.unpack_from(">HH", m.payload, 0)
        mk = mark_at(timeline, i) or "(pre-mark)"
        tab[mk][{0x02: "handshake", 0x04: "data", 0x08: "ack", 0x0C: "data+ack",
                 0x10: "keepalive", 0x14: "data+ka", 0x18: "ack+ka"}.get(typ, hex(typ))] += 1
        if (typ & 0x04) and len(m.payload) > 8:
            tab[mk]["data-bytes"] += len(m.payload) - 8
    return ["(pre-mark)"] + order, tab


def records(path):
    """Every decoded application message in the capture, in wire order, with marks."""
    msgs = load(path)
    timeline = mark_timeline(msgs)
    out = []
    for (src, dst), (stream, index) in streams(msgs).items():
        host_is_dst = dst < 0x1000
        chan = src if host_is_dst else dst  # always name the channel by device node
        for pos, hdr, obj, err in walk(stream):
            _off, usb, t = locate(index, pos)
            out.append(
                Rec(chan, not host_is_dst, pos, hdr, obj, err, t, usb,
                    mark_at(timeline, usb))
            )
            if err:
                break
    out.sort(key=lambda r: (r.usb, r.pos))
    # Responses carry no opcode of their own; pair each with its request so a
    # caller can filter on "everything to do with opcode N".
    txn_op = {(r.chan, r.txn): r.opcode for r in out if r.kind == "req"}
    for r in out:
        r.for_op = txn_op.get((r.chan, r.txn)) if r.kind == "rsp" else r.opcode
    return out


# ----------------------------------------------------------------- rendering ---
def inventory(paths):
    """Opcode and notification inventory across several captures.

    This is what `docs/_reference/opcodes.md` is generated from: for each opcode, every
    distinct argument/result *shape* with one real example and the marks that
    produced it; for each notification, the (82, 68, 121) tag tuple it carries.
    """
    ops, evs = collections.defaultdict(list), collections.defaultdict(list)
    for p in paths:
        tag = p.split("/")[-1]
        recs = records(p)
        rsps = {(r.chan, r.txn): r for r in recs if r.kind == "rsp"}
        for r in recs:
            if r.kind == "req":
                ops[r.opcode].append((tag, r, rsps.get((r.chan, r.txn))))
            elif r.kind == "note":
                a = r.obj.get(106)
                key = (r.event,
                       a.get(82) if isinstance(a, dict) else None,
                       a.get(68) if isinstance(a, dict) else None,
                       a.get(121) if isinstance(a, dict) else None)
                evs[key].append((tag, r))
    return ops, evs


def _shape(o, d=0):
    if isinstance(o, Blob):
        return f"<blob {len(o)}B>"
    if isinstance(o, dict):
        return "{...}" if d >= 2 else "{" + ", ".join(
            f"{k}: {_shape(v, d+1)}" for k, v in o.items()) + "}"
    if isinstance(o, list):
        if not o:
            return "[]"
        return f"[{_shape(o[0], d+1)} x{len(o)}]" if d >= 2 or len(o) > 4 else \
            "[" + ", ".join(_shape(v, d + 1) for v in o) + "]"
    return {bool: "bool", float: "float", int: "int", str: "str",
            type(None): "nil"}.get(type(o), type(o).__name__)


def render(o, depth=6, d=0):
    if isinstance(o, Blob):
        inner = o.nested()
        if inner is None:
            return f"<blob {len(o)}B {o[:16].hex(' ')}...>"
        if d >= depth:
            return f"<nested {len(o)}B>"
        return f"<nested {len(o)}B>[" + ", ".join(render(v, depth, d + 1) for v in inner) + "]"
    if isinstance(o, dict):
        if d >= depth:
            return f"{{...{len(o)} keys...}}"
        return "{" + ", ".join(f"{k}: {render(v, depth, d+1)}" for k, v in o.items()) + "}"
    if isinstance(o, list):
        if d >= depth:
            return f"[...{len(o)} items...]"
        if len(o) > 40:
            return "[" + ", ".join(render(v, depth, d + 1) for v in o[:40]) + f", ...+{len(o)-40}]"
        return "[" + ", ".join(render(v, depth, d + 1) for v in o) + "]"
    if isinstance(o, float):
        return f"{o:g}"
    return repr(o)


def line(r, depth):
    arrow = "-->" if r.out else "<--"
    tag = {"req": f"op{r.opcode}", "rsp": "rsp", "note": f"ev{r.event}"}.get(r.kind, "?")
    body = r.err or render(r.obj, depth)
    return f"  {r.t:9.3f} {r.chan:#06x} {arrow} {tag:>6}  {body}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log", nargs="+", help="capture logs (several only with --dict)")
    ap.add_argument("--dict", action="store_true", dest="dic",
                    help="opcode + notification inventory across all given logs")
    ap.add_argument("--mark", help="only marks containing this substring")
    ap.add_argument("--chan", help="device node, e.g. 0x1002")
    ap.add_argument("--opcode", type=int, action="append",
                    help="only this opcode (and its reply); repeatable")
    ap.add_argument("--event", type=int, action="append",
                    help="only this notification event id; repeatable")
    ap.add_argument("--kind", choices=("req", "rsp", "note"))
    ap.add_argument("--depth", type=int, default=6)
    ap.add_argument("--summary", action="store_true", help="opcode/event x mark cross-tab")
    ap.add_argument("--marks", action="store_true",
                    help="every mark, including ones with no traffic at all")
    args = ap.parse_args()

    if args.dic:
        ops, evs = inventory(args.log)
        print("################ opcodes ################")
        for op in sorted(ops):
            print(f"\n--- opcode {op}   ({len(ops[op])} calls) ---")
            seen = set()
            for tag, r, rsp in ops[op]:
                sig = (_shape(r.args), _shape(rsp.obj.get(104)) if rsp else None,
                       rsp.obj.get(103) if rsp else None)
                if sig in seen:
                    continue
                seen.add(sig)
                print(f"  chan {r.chan:#06x}  mark={r.mark}  [{tag}]")
                print(f"    --> {render(r.args, 3)}"[:300])
                print(f"    <-- 103={rsp.obj.get(103)} 104={render(rsp.obj.get(104), 3)}"[:300]
                      if rsp else "    <-- (no reply seen)")
            print("    marks: " + ", ".join(
                sorted({str(r.mark) for _, r, _ in ops[op]})))
        print("\n################ notifications ################")
        for key in sorted(evs, key=lambda k: tuple(-1 if x is None else x for x in k)):
            items = evs[key]
            print(f"\n--- 105={key[0]}  82={key[1]} 68={key[2]} 121={key[3]}  ({len(items)}x) ---")
            print("    marks: " + ", ".join(sorted({str(r.mark) for _, r in items})[:8]))
            seen = set()
            for tag, r in items:
                s = _shape(r.args)
                if s in seen:
                    continue
                seen.add(s)
                print(f"    args {render(r.args, 3)}"[:260])
        return

    if len(args.log) > 1:
        ap.error("only --dict takes more than one log")
    args.log = args.log[0]

    if args.marks:
        order, tab = raw_activity(args.log)
        per_mark = collections.defaultdict(list)
        for r in records(args.log):
            per_mark[r.mark or "(pre-mark)"].append(r)
        print(f"{'mark':<32} {'msgs':>5} {'data':>5} {'bytes':>7}  what")
        for mk in order:
            rs = per_mark.get(mk, [])
            what = " ".join(
                sorted({f"op{r.opcode}" if r.kind == "req"
                        else f"ev{r.event}" if r.kind == "note" else "rsp" for r in rs})
            )
            print(f"{mk:<32} {len(rs):5} {tab[mk]['data']:5} "
                  f"{tab[mk]['data-bytes']:7}  {what or '-- no application traffic --'}")
        return

    recs = records(args.log)

    if args.mark:
        recs = [r for r in recs if r.mark and args.mark in r.mark]
    if args.chan:
        want = int(args.chan, 0)
        recs = [r for r in recs if r.chan == want]
    if args.opcode:
        recs = [r for r in recs if r.for_op in args.opcode]
    if args.event:
        recs = [r for r in recs if r.event in args.event]
    if args.kind:
        recs = [r for r in recs if r.kind == args.kind]

    if args.summary:
        tab = collections.Counter()
        for r in recs:
            what = (f"op{r.opcode}" if r.kind == "req"
                    else f"ev{r.event}" if r.kind == "note" else None)
            if what:
                tab[(r.mark or "(pre-mark)", what)] += 1
        cur = None
        for (mk, what), n in sorted(tab.items(), key=lambda x: (x[0][0], x[0][1])):
            if mk != cur:
                print(f"\n=== {mk} ===")
                cur = mk
            print(f"  {what:>8}  x{n}")
        return

    cur = object()
    for r in recs:
        if r.mark != cur:
            print(f"\n=== {r.mark or '(pre-mark)'} ===")
            cur = r.mark
        print(line(r, args.depth))


if __name__ == "__main__":
    main()
