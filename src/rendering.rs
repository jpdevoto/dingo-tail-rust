// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RendererMode {
    #[default]
    Auto,
    Hardware,
    Software,
}

pub fn mode_from_args(
    mode: RendererMode,
    args: impl IntoIterator<Item = String>,
) -> Result<RendererMode, String> {
    let mut mode = mode;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let value = if arg == "--renderer" {
            Some(
                args.next()
                    .ok_or("--renderer requires auto, hardware, or software")?,
            )
        } else {
            arg.strip_prefix("--renderer=").map(str::to_owned)
        };
        if let Some(value) = value {
            mode = match value.as_str() {
                "auto" => RendererMode::Auto,
                "hardware" => RendererMode::Hardware,
                "software" => RendererMode::Software,
                _ => return Err(format!("Unknown renderer: {value}")),
            };
        }
    }
    Ok(mode)
}

static SOFTWARE: AtomicBool = AtomicBool::new(false);
static MINIMIZED: AtomicBool = AtomicBool::new(false);

pub fn set_minimized(value: bool) {
    MINIMIZED.store(value, Ordering::Relaxed);
}

fn log_interval(software: bool, minimized: bool) -> Duration {
    Duration::from_millis(if minimized {
        1000
    } else if software {
        80
    } else {
        40
    })
}

pub fn request_log_repaint(ctx: &eframe::egui::Context) {
    // egui keeps the earliest deadline: later batches never postpone this wakeup.
    // No timer runs once incoming data stops. Input/status events remain immediate.
    // egui subtracts predicted_dt from delayed requests. Compensate so a 40 ms
    // data budget does not become ~23 ms on a 60 Hz display.
    let prediction = ctx.input(|i| Duration::try_from_secs_f32(i.predicted_dt).unwrap_or_default());
    ctx.request_repaint_after(
        log_interval(
            SOFTWARE.load(Ordering::Relaxed),
            MINIMIZED.load(Ordering::Relaxed),
        ) + prediction,
    );
}

pub fn diagnostic(message: impl std::fmt::Display) {
    eprintln!("rendering: {message}");
    if let Some(path) = std::env::var_os("DINGOTAIL_RENDER_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(
            file,
            "{:?} rendering: {message}",
            std::time::SystemTime::now()
        );
    }
}

pub fn is_remote_session() -> bool {
    #[cfg(windows)]
    {
        // GetSystemMetrics does not retain pointers or require initialization.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows_sys::Win32::UI::WindowsAndMessaging::SM_REMOTESESSION,
            ) != 0
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
pub fn configure_dx12(options: &mut eframe::NativeOptions, mode: RendererMode) {
    options.renderer = eframe::Renderer::Wgpu;
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    setup.instance_descriptor.backends = wgpu::Backends::DX12;
    setup.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, surface| {
        let adapter = adapters
            .iter()
            .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
            .filter(|a| {
                mode != RendererMode::Software || a.get_info().device_type == wgpu::DeviceType::Cpu
            })
            .min_by_key(|a| match a.get_info().device_type {
                wgpu::DeviceType::DiscreteGpu => 0,
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::VirtualGpu => 2,
                wgpu::DeviceType::Other => 3,
                wgpu::DeviceType::Cpu => 4,
            })
            .ok_or_else(|| format!("No surface-compatible DX12 adapter for {mode:?}"))?;
        let info = adapter.get_info();
        SOFTWARE.store(info.device_type == wgpu::DeviceType::Cpu, Ordering::Relaxed);
        diagnostic(format!(
            "DX12 adapter={} type={:?} driver={} {}",
            info.name, info.device_type, info.driver, info.driver_info
        ));
        if info.device_type == wgpu::DeviceType::Cpu && mode != RendererMode::Software {
            diagnostic("No preferred hardware adapter available; using CPU adapter");
        }
        Ok(adapter.clone())
    }));
    options.wgpu_options.wgpu_setup = setup.into();
}

pub fn initialized(cc: &eframe::CreationContext<'_>) -> bool {
    if let Some(gl) = &cc.gl {
        use eframe::glow::HasContext;
        // The creation callback runs with the GL context current.
        let renderer = unsafe { gl.get_parameter_string(eframe::glow::RENDERER) };
        let lower = renderer.to_lowercase();
        SOFTWARE.store(
            [
                "llvmpipe",
                "softpipe",
                "software",
                "gdi generic",
                "swiftshader",
            ]
            .iter()
            .any(|s| lower.contains(s)),
            Ordering::Relaxed,
        );
        diagnostic(format!("OpenGL renderer={renderer}"));
    }
    SOFTWARE.load(Ordering::Relaxed)
}

pub struct FrameStats {
    start: Instant,
    frames: u64,
    samples: u64,
    cpu_seconds: f64,
    enabled: bool,
}
impl Default for FrameStats {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            frames: 0,
            samples: 0,
            cpu_seconds: 0.,
            enabled: std::env::var_os("DINGOTAIL_RENDER_LOG").is_some(),
        }
    }
}
impl FrameStats {
    pub fn record(&mut self, cpu: Option<f32>) {
        if !self.enabled {
            return;
        }
        self.frames += 1;
        if let Some(cpu) = cpu {
            self.samples += 1;
            self.cpu_seconds += f64::from(cpu);
        }
        let elapsed = self.start.elapsed().as_secs_f64();
        if elapsed >= 10. {
            diagnostic(format!(
                "updates_per_second={:.2} average_eframe_cpu_ms={:.3} samples={} (excludes GPU/present time)",
                self.frames as f64 / elapsed,
                1000. * self.cpu_seconds / self.samples.max(1) as f64,
                self.samples
            ));
            self.start = Instant::now();
            self.frames = 0;
            self.samples = 0;
            self.cpu_seconds = 0.;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_configuration() {
        assert_eq!(
            serde_json::from_str::<crate::model::Settings>("{}")
                .unwrap()
                .renderer,
            RendererMode::Auto
        );
        assert_eq!(
            serde_json::from_str::<RendererMode>("\"software\"").unwrap(),
            RendererMode::Software
        );
        assert_eq!(
            mode_from_args(RendererMode::Software, ["--renderer=auto".into()]).unwrap(),
            RendererMode::Auto
        );
        assert_eq!(
            mode_from_args(RendererMode::Auto, ["--renderer".into(), "hardware".into()]).unwrap(),
            RendererMode::Hardware
        );
        assert!(mode_from_args(RendererMode::Auto, ["--renderer".into()]).is_err());
        assert!(mode_from_args(RendererMode::Auto, ["--renderer=invalid".into()]).is_err());
    }
    #[test]
    fn bursts_schedule_one_delayed_wakeup() {
        let ctx = eframe::egui::Context::default();
        for _ in 0..3 {
            let _ = ctx.run(Default::default(), |_| {});
        }
        let delays = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = delays.clone();
        ctx.set_request_repaint_callback(move |request| {
            observed.lock().unwrap().push(request.delay)
        });
        for _ in 0..100 {
            request_log_repaint(&ctx);
        }
        assert_eq!(*delays.lock().unwrap(), vec![Duration::from_millis(40)]);
        let _ = ctx.run(Default::default(), |_| {});
        let idle = ctx.run(Default::default(), |_| {});
        assert_eq!(
            idle.viewport_output[&eframe::egui::ViewportId::ROOT].repaint_delay,
            Duration::MAX
        );
    }

    #[test]
    fn data_refresh_budget() {
        assert_eq!(log_interval(false, false), Duration::from_millis(40));
        assert_eq!(log_interval(true, false), Duration::from_millis(80));
        assert_eq!(log_interval(false, true), log_interval(true, true));
        assert_eq!(log_interval(true, true), Duration::from_secs(1));
    }
}
