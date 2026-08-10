// midiprobe - a small CoreMIDI probe for reverse-engineering device protocols.
//
//   midiprobe list
//   midiprobe listen <src-substring> [ms]
//   midiprobe send <dst-substring> <src-substring> <hex...> [--wait ms]
//
// Hex bytes may be written as "F0 7E 7F 06 01 F7" or "F07E7F0601F7".

import Foundation
import CoreMIDI

// MARK: - Formatting

func stringProperty(_ obj: MIDIObjectRef, _ prop: CFString) -> String {
    var out: Unmanaged<CFString>?
    guard MIDIObjectGetStringProperty(obj, prop, &out) == noErr, let v = out else { return "" }
    return v.takeRetainedValue() as String
}

func hex(_ bytes: [UInt8]) -> String {
    bytes.map { String(format: "%02X", $0) }.joined(separator: " ")
}

func printable(_ bytes: [UInt8]) -> String {
    String(bytes.map { $0 >= 0x20 && $0 < 0x7F ? Character(UnicodeScalar($0)) : "." })
}

/// Hex dump with an ASCII gutter, 16 bytes per line.
func dump(_ bytes: [UInt8], indent: String = "  ") {
    var offset = 0
    while offset < bytes.count {
        let slice = Array(bytes[offset..<min(offset + 16, bytes.count)])
        let hexPart = hex(slice).padding(toLength: 47, withPad: " ", startingAt: 0)
        print("\(indent)\(String(format: "%04X", offset))  \(hexPart)  |\(printable(slice))|")
        offset += 16
    }
}

// MARK: - Port lookup

func sources() -> [(Int, MIDIEndpointRef, String)] {
    (0..<MIDIGetNumberOfSources()).map { i in
        let e = MIDIGetSource(i)
        return (i, e, stringProperty(e, kMIDIPropertyDisplayName))
    }
}

func destinations() -> [(Int, MIDIEndpointRef, String)] {
    (0..<MIDIGetNumberOfDestinations()).map { i in
        let e = MIDIGetDestination(i)
        return (i, e, stringProperty(e, kMIDIPropertyDisplayName))
    }
}

func match(_ list: [(Int, MIDIEndpointRef, String)], _ needle: String) -> (MIDIEndpointRef, String)? {
    let n = needle.lowercased()
    for (_, ref, name) in list where name.lowercased().contains(n) { return (ref, name) }
    return nil
}

// MARK: - Receive plumbing

final class Collector {
    private let lock = NSLock()
    private var buffer: [UInt8] = []
    private let start = Date()

    func append(_ bytes: [UInt8]) {
        lock.lock()
        let elapsed = Date().timeIntervalSince(start)
        print(String(format: "\n[%7.3fs] RX %d bytes", elapsed, bytes.count))
        dump(bytes)
        buffer.append(contentsOf: bytes)
        lock.unlock()
    }

    /// Pull complete F0..F7 SysEx messages out of everything received so far.
    func sysexMessages() -> [[UInt8]] {
        lock.lock(); defer { lock.unlock() }
        var out: [[UInt8]] = []
        var current: [UInt8]? = nil
        for b in buffer {
            if b == 0xF0 { current = [b] }
            else if var c = current {
                c.append(b)
                if b == 0xF7 { out.append(c); current = nil } else { current = c }
            }
        }
        return out
    }

    var isEmpty: Bool { lock.lock(); defer { lock.unlock() }; return buffer.isEmpty }
}

func makeClientAndInput(_ collector: Collector) -> (MIDIClientRef, MIDIPortRef) {
    var client = MIDIClientRef()
    MIDIClientCreateWithBlock("midiprobe" as CFString, &client, nil)

    var inPort = MIDIPortRef()
    MIDIInputPortCreateWithBlock(client, "midiprobe-in" as CFString, &inPort) { listPtr, _ in
        let list = listPtr.pointee
        var packet = list.packet
        for _ in 0..<list.numPackets {
            let length = Int(packet.length)
            let bytes: [UInt8] = withUnsafeBytes(of: packet.data) { raw in
                Array(raw.bindMemory(to: UInt8.self).prefix(length))
            }
            if !bytes.isEmpty { collector.append(bytes) }
            packet = MIDIPacketNext(&packet).pointee
        }
    }
    return (client, inPort)
}

func send(_ outPort: MIDIPortRef, _ dest: MIDIEndpointRef, _ message: [UInt8]) {
    let bufSize = 65536
    let raw = UnsafeMutableRawPointer.allocate(
        byteCount: bufSize, alignment: MemoryLayout<MIDIPacketList>.alignment)
    defer { raw.deallocate() }
    let list = raw.bindMemory(to: MIDIPacketList.self, capacity: 1)
    var cur = MIDIPacketListInit(list)
    cur = MIDIPacketListAdd(list, bufSize, cur, 0, message.count, message)
    let status = MIDISend(outPort, dest, list)
    if status != noErr { print("!! MIDISend failed: \(status)") }
}

func manufacturerName(_ id: [UInt8]) -> String {
    if id == [0x00, 0x01, 0x0C] { return "Line 6" }
    return "unknown"
}

/// Annotate a complete SysEx message with whatever structure we recognise.
func decode(_ m: [UInt8]) {
    guard m.count > 4 else { return }

    if m[1] == 0x7E || m[1] == 0x7F {
        let realtime = m[1] == 0x7F
        print("  universal \(realtime ? "realtime" : "non-realtime"), device ID \(hex([m[2]]))")

        // Identity Reply: F0 7E <dev> 06 02 <mfg> <family:2> <member:2> <rev:4> F7
        if m[3] == 0x06 && m[4] == 0x02 && m.count >= 15 {
            let extended = m[5] == 0x00
            let mfg = extended ? [m[5], m[6], m[7]] : [m[5]]
            let p = 5 + mfg.count
            print("  Identity Reply")
            print("    manufacturer  \(hex(mfg))  (\(manufacturerName(mfg)))")
            guard m.count >= p + 8 else { return }
            let family = Int(m[p]) | (Int(m[p + 1]) << 7)
            let member = Int(m[p + 2]) | (Int(m[p + 3]) << 7)
            let rev = Array(m[(p + 4)..<(p + 8)])
            print("    family        0x\(String(format: "%04X", family)) (\(family))")
            print("    member        0x\(String(format: "%04X", member)) (\(member))")
            print("    device key    0x\(String(format: "%04X%04X", family, member))"
                  + "   (matches the key HX Edit.prefs uses)")
            // The revision encoding is not yet pinned down: 0x50 reads as either
            // 80 (plain decimal) or "50" (BCD). Show both until confirmed.
            print("    revision      \(hex(rev))")
            print("      as decimal  \(rev[0]).\(String(format: "%02d", rev[1]))")
            print("      as BCD      \(String(format: "%x", rev[0])).\(String(format: "%02x", rev[1]))")
        }
        return
    }

    if m[1] == 0x00 {
        let mfg = [m[1], m[2], m[3]]
        print("  manufacturer  \(hex(mfg))  (\(manufacturerName(mfg)))")
        print("  payload       \(hex(Array(m[4..<(m.count - 1)])))")
    } else {
        print("  manufacturer  \(hex([m[1]]))  (single-byte ID)")
    }
}

func summarize(_ collector: Collector) {
    let messages = collector.sysexMessages()
    print("\n--- summary ---")
    if messages.isEmpty {
        print("No complete SysEx messages received.")
        return
    }
    for (i, m) in messages.enumerated() {
        print("\nSysEx #\(i + 1)  (\(m.count) bytes)")
        dump(m)
        decode(m)
    }
}

// MARK: - Commands

func parseHex(_ args: [String]) -> [UInt8] {
    let joined = args.joined().replacingOccurrences(of: "0x", with: "")
        .filter { $0.isHexDigit }
    var bytes: [UInt8] = []
    var i = joined.startIndex
    while i < joined.endIndex {
        let j = joined.index(i, offsetBy: 2, limitedBy: joined.endIndex) ?? joined.endIndex
        if let b = UInt8(joined[i..<j], radix: 16) { bytes.append(b) }
        i = j
    }
    return bytes
}

let argv = CommandLine.arguments
guard argv.count > 1 else {
    print("""
    usage:
      midiprobe list
      midiprobe listen <src-substring> [ms]
      midiprobe send <dst-substring> <src-substring> <hex...> [--wait ms]
    """)
    exit(1)
}

switch argv[1] {
case "list":
    print("Sources (device -> Mac):")
    for (i, _, name) in sources() { print("  [\(i)] \(name)") }
    print("\nDestinations (Mac -> device):")
    for (i, _, name) in destinations() { print("  [\(i)] \(name)") }

case "listen":
    guard argv.count > 2 else { print("need a source substring"); exit(1) }
    let ms = argv.count > 3 ? Int(argv[3]) ?? 5000 : 5000
    guard let (src, name) = match(sources(), argv[2]) else {
        print("no source matching '\(argv[2])'"); exit(1)
    }
    let collector = Collector()
    let (_, inPort) = makeClientAndInput(collector)
    MIDIPortConnectSource(inPort, src, nil)
    print("Listening on '\(name)' for \(ms) ms - interact with the device/app now...")
    RunLoop.current.run(until: Date().addingTimeInterval(Double(ms) / 1000))
    if collector.isEmpty { print("\nNothing received.") }
    summarize(collector)

case "send":
    guard argv.count > 4 else { print("need <dst> <src> <hex...>"); exit(1) }
    var rest = Array(argv[4...])
    var waitMs = 3000
    if let idx = rest.firstIndex(of: "--wait"), idx + 1 < rest.count {
        waitMs = Int(rest[idx + 1]) ?? 3000
        rest.removeSubrange(idx...(idx + 1))
    }
    let message = parseHex(rest)
    guard !message.isEmpty else { print("no hex bytes given"); exit(1) }
    guard let (dst, dstName) = match(destinations(), argv[2]) else {
        print("no destination matching '\(argv[2])'"); exit(1)
    }
    guard let (src, srcName) = match(sources(), argv[3]) else {
        print("no source matching '\(argv[3])'"); exit(1)
    }

    let collector = Collector()
    let (client, inPort) = makeClientAndInput(collector)
    MIDIPortConnectSource(inPort, src, nil)
    var outPort = MIDIPortRef()
    MIDIOutputPortCreate(client, "midiprobe-out" as CFString, &outPort)

    print("TX -> '\(dstName)'  (listening on '\(srcName)')")
    dump(message)
    // Give the input port a moment to settle before transmitting.
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
    send(outPort, dst, message)
    RunLoop.current.run(until: Date().addingTimeInterval(Double(waitMs) / 1000))
    if collector.isEmpty { print("\nNo response.") }
    summarize(collector)

default:
    print("unknown command '\(argv[1])'")
    exit(1)
}
