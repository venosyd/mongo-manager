// No Windows, a versão release abre sem janela de console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod completion;
mod config;
mod dialogs;
mod editor;
mod grid;
mod mongo;
mod script;
mod shell;
mod views;

fn app_icon() -> egui::IconData {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../assets/icon-256.png")));
    let mut reader = decoder.read_info().expect("ícone PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("tamanho do ícone")];
    let info = reader.next_frame(&mut rgba).expect("ícone PNG");
    rgba.truncate(info.buffer_size());
    egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mongo Manager")
            .with_app_id("mongo-manager")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "mongo-manager",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

#[cfg(test)]
mod glyph_tests {
    /// Todo caractere não-ASCII usado na interface precisa existir nas fontes embutidas.
    #[test]
    fn all_ui_glyphs_exist() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let mut missing = std::collections::BTreeSet::new();
        for file in std::fs::read_dir("src").unwrap() {
            let src = std::fs::read_to_string(file.unwrap().path()).unwrap();
            for c in src.chars().filter(|c| !c.is_ascii() && !c.is_alphabetic() && *c != '‸') {
                let font = egui::FontId::proportional(14.0);
                if !ctx.fonts_mut(|f| f.has_glyph(&font, c)) {
                    missing.insert(c);
                }
            }
        }
        assert!(missing.is_empty(), "glifos ausentes: {missing:?}");
    }
}

