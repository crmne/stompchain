// Send MIDI beat clock (0xF8) at a given BPM for a given duration.
import Foundation
import CoreMIDI
var client = MIDIClientRef(); MIDIClientCreate("clk" as CFString, nil, nil, &client)
var port = MIDIPortRef(); MIDIOutputPortCreate(client, "out" as CFString, &port)
var dest: MIDIEndpointRef = 0
for i in 0..<MIDIGetNumberOfDestinations() {
    let d = MIDIGetDestination(i)
    var name: Unmanaged<CFString>?
    MIDIObjectGetStringProperty(d, kMIDIPropertyDisplayName, &name)
    if let n = name?.takeRetainedValue() as String?, n.contains("HX Stomp") { dest = d }
}
guard dest != 0 else { print("no HX Stomp"); exit(1) }
let bpm = 120.0, seconds = Double(CommandLine.arguments.count > 1 ? Int(CommandLine.arguments[1])! : 20)
let interval = 60.0 / bpm / 24.0
print("clocking \(bpm) BPM for \(seconds)s")
let start = Date()
// Start message, then a steady stream of clocks.
for byte: [UInt8] in [[0xFA]] {
    var pkt = MIDIPacketList(); var cur = MIDIPacketListInit(&pkt)
    cur = MIDIPacketListAdd(&pkt, 1024, cur, 0, byte.count, byte)
    MIDISend(port, dest, &pkt)
}
while Date().timeIntervalSince(start) < seconds {
    var pkt = MIDIPacketList(); var cur = MIDIPacketListInit(&pkt)
    let b: [UInt8] = [0xF8]
    cur = MIDIPacketListAdd(&pkt, 1024, cur, 0, 1, b)
    MIDISend(port, dest, &pkt)
    Thread.sleep(forTimeInterval: interval)
}
print("done")
