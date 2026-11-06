// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod credentials;
mod local;
mod model;
mod network;
#[cfg(test)]
mod performance_tests;
mod platform;
mod rendering;
mod theme;
mod ui;
fn main() -> eframe::Result<()> {
    #[allow(unused_mut)]
    let mut size = [1200., 780.];
    #[cfg(feature = "qa")]
    if let Ok(scale) = std::env::var("SSHLOGTAIL_QA_SCALE") {
        let scale: f32 = scale.parse().unwrap_or(1.);
        size = [1200f32.max(800. * scale), 780f32.max(500. * scale)];
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(theme::window_icon())
            .with_inner_size(size)
            .with_min_inner_size([800., 500.]),
        ..Default::default()
    };
    let mode = match rendering::mode_from_args(
        model::Settings::load()
            .map(|s| s.renderer)
            .unwrap_or_default(),
        std::env::args().skip(1),
    ) {
        Ok(mode) => mode,
        Err(error) => {
            startup_error(&error);
            return Err(eframe::Error::AppCreation(error.into()));
        }
    };
    let remote = rendering::is_remote_session();
    rendering::diagnostic(format!("requested={mode:?} remote_session={remote}"));
    let dx12 = std::env::args_os().skip(1).any(|arg| arg == "--dx12");
    if let Err(error) = platform::validate_renderer(
        cfg!(windows),
        mode == rendering::RendererMode::Software,
        dx12,
    ) {
        startup_error(&error);
        return Err(eframe::Error::AppCreation(error.into()));
    }
    #[cfg(windows)]
    let use_dx12 = dx12 || remote || mode == rendering::RendererMode::Software;
    #[cfg(not(windows))]
    let use_dx12 = dx12;
    #[cfg(windows)]
    if use_dx12 {
        rendering::configure_dx12(&mut options, mode);
    }
    let initialized = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let run = |options, reject_software_gl: bool| {
        let ready = initialized.clone();
        eframe::run_native(
            "DingoTail",
            options,
            Box::new(move |cc| {
                let software = rendering::initialized(cc);
                #[cfg(windows)]
                if software && reject_software_gl {
                    return Err("OpenGL reports a software renderer; trying DX12".into());
                }
                #[cfg(not(windows))]
                let _ = (software, reject_software_gl);
                ready.store(true, std::sync::atomic::Ordering::Relaxed);
                Ok(Box::new(ui::App::new(cc)))
            }),
        )
    };
    #[allow(unused_mut)]
    let mut result = run(
        options.clone(),
        !use_dx12 && mode == rendering::RendererMode::Auto,
    );
    // Only retry startup failures, never restart a running application after an error.
    if let Err(error) = &result {
        rendering::diagnostic(format!("initialization/run failure: {error}"));
    }
    #[cfg(windows)]
    if result.is_err()
        && !initialized.load(std::sync::atomic::Ordering::Relaxed)
        && mode == rendering::RendererMode::Auto
    {
        if use_dx12 {
            rendering::diagnostic("DX12 initialization failed; trying OpenGL");
            options.renderer = eframe::Renderer::Glow;
        } else {
            rendering::diagnostic("OpenGL initialization failed; trying DX12 hardware/CPU adapter");
            rendering::configure_dx12(&mut options, mode);
        }
        result = run(options.clone(), false);
        if !use_dx12 && result.is_err() && !initialized.load(std::sync::atomic::Ordering::Relaxed) {
            rendering::diagnostic(format!(
                "DX12 fallback failed: {:?}; last attempt with OpenGL including software drivers",
                result.as_ref().err()
            ));
            options.renderer = eframe::Renderer::Glow;
            result = run(options, false);
        }
    }
    #[cfg(not(windows))]
    let _ = use_dx12;
    if let Err(error) = &result {
        let guidance = if cfg!(windows) {
            "Try --renderer auto, --dx12, or --renderer software (Windows DX12 CPU adapter required)."
        } else {
            "Check your OpenGL driver and graphical desktop session. Use --renderer auto or hardware."
        };
        startup_error(&format!("{error}\n\n{guidance}"));
    }
    result
}

fn startup_error(message: &str) {
    rendering::diagnostic(message);
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("DingoTail renderer could not start")
        .set_description(message)
        .show();
}
