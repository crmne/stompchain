//! Scratch probe: what statuses do deliberately bad requests earn?
use hx_proto::msgpack::Value;
use hx_proto::ChannelId;

#[test]
#[ignore = "probe"]
fn statuses_of_bad_requests() {
    let Some(found) = hx_usb::list().unwrap().into_iter().next() else { return };
    let mut s = found.open().expect("open");
    let (_, started_at, _) = s.preset_info().expect("current");

    let mut go = |label: &str, id: ChannelId, op: i64, args: Value| {
        match s.request_full(id, op, args) {
            Ok((status, result)) => {
                let r = format!("{result:?}");
                println!("{label:<28} status {status} result {}", &r[..r.len().min(60)]);
            }
            Err(e) => println!("{label:<28} ERR {e}"),
        }
        // Health after every poke; stop early if something died.
        s.preset_info().expect("health");
    };

    go("select preset 999", ChannelId::DATA, 20,
       hx_proto::msgmap! { 107 => Value::Int(0), 108 => Value::Int(999) });
    go("select snapshot 7", ChannelId::DATA, 88,
       hx_proto::msgmap! { 92 => Value::Int(7) });
    go("param on empty slot 5", ChannelId::DATA, 30,
       hx_proto::msgmap! { 98 => Value::Int(5), 28 => Value::Int(0), 119 => Value::F32(0.5), 29 => Value::Bool(true) });
    go("param index 99 on block 1", ChannelId::DATA, 30,
       hx_proto::msgmap! { 98 => Value::Int(1), 28 => Value::Int(99), 119 => Value::F32(0.5), 29 => Value::Bool(true) });
    go("bad model 99999", ChannelId::DATA, 40,
       hx_proto::msgmap! { 98 => Value::Int(1), 100 => hx_proto::msgmap! {
           23 => Value::Bool(false), 25 => Value::Int(99999), 26 => Value::Int(-1) } });
    go("clear empty IR slot 60", ChannelId::CONTROL, 15,
       hx_proto::msgmap! { 112 => Value::Int(60) });
    go("select preset -2", ChannelId::DATA, 20,
       hx_proto::msgmap! { 107 => Value::Int(0), 108 => Value::Int(-2) });

    // Restore and verify.
    s.select_preset(0, started_at).expect("restore preset");
    s.irs().expect("health control");
    println!("healthy");
}
