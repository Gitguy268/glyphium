//! Renders `fixtures/render-demo.bin` offscreen with the system GPU and writes a PNG, for
//! comparing with the same frame rendered in the browser (`bench/web/run-render.mjs`).
//! Usage: render_png OUT.png [INPUT.bin] [--theme NAME]

use glyphium_terminal_core::{Frame, Terminal, Theme};
use glyphium_terminal_render::{FontConfig, OFFSCREEN_FORMAT, Renderer};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let theme = args.iter().position(|a| a == "--theme").map(|i| {
        args.remove(i);
        let name = args.remove(i);
        Theme::by_name(&name).unwrap_or_else(|| panic!("unknown theme {name}; have {:?}", Theme::NAMES))
    });
    let out = args.first().expect("usage: render_png OUT.png [INPUT.bin] [--theme NAME]");
    let input = args.get(1).map_or("fixtures/render-demo.bin", String::as_str);
    let bytes = std::fs::read(input).expect("run from the repo root");
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    eprintln!("adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    let fonts = FontConfig {
        size_px: 16.0,
        extra_fonts: vec![std::fs::read("fixtures/fonts/NotoSansMonoCJKjp-subset.otf").unwrap(), std::fs::read("fixtures/fonts/NotoColorEmoji-subset.ttf").unwrap()],
        ..Default::default()
    };
    let mut renderer = Renderer::new(device, queue, OFFSCREEN_FORMAT, &fonts);
    let (cols, rows) = (80, 24);
    let mut term = Terminal::new(cols, rows, 1000);
    if let Some(theme) = &theme {
        term.set_theme(Some(theme));
    }
    term.write(&bytes);
    let mut frame = Frame::default();
    term.update_frame(&mut frame);
    let mut view = renderer.new_view();
    renderer.update(&mut view, &frame);
    let (w, h) = renderer.pixel_size(cols, rows);
    let rgba = pollster::block_on(renderer.render_to_rgba(&view, frame.background, (w, h)));
    // Raw pixels next to the PNG, for exact comparison.
    std::fs::write(format!("{out}.rgba"), &rgba).unwrap();
    let file = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    eprintln!("{w}x{h} {:?} metrics {:?}", renderer.stats, renderer.metrics());
}
