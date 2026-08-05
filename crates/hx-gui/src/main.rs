//! Desktop entry point.

fn main() -> eframe::Result<()> {
    let (tx, rx) = hx_gui::spawn();
    // Closing the window must let the device go cleanly. A process that just
    // disappears leaves the device mid-conversation, and it then refuses new
    // sessions until its power is pulled.
    let on_exit = tx.clone();
    eframe::run_native(
        "stompchain",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 640.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            // Lets `ui.image("file://…")` load the model artwork HX Edit ships.
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(hx_gui::App::new(&cc.egui_ctx, tx, rx)))
        }),
    )?;

    // eframe has returned, so the window is gone; give the worker a moment to
    // put the device down before the process exits.
    let _ = on_exit.send(hx_gui::Cmd::Disconnect);
    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(())
}
