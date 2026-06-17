use anyhow::{Context, Result};
use image::RgbaImage;
use std::time::Duration;
use xcap::Monitor;

pub struct ScreenCapturer {
    monitor: Monitor,
    width: u32,
    height: u32,
    scale_factor: f64,
}

impl ScreenCapturer {
    pub fn new(monitor_index: usize) -> Result<Self> {
        let monitors = Monitor::all().context("Failed to enumerate monitors")?;
        if monitor_index >= monitors.len() {
            anyhow::bail!(
                "Monitor index {} out of range ({} monitors)",
                monitor_index,
                monitors.len()
            );
        }
        let monitor = monitors[monitor_index].clone();
        let width = monitor.width();
        let height = monitor.height();
        let scale_factor = Self::detect_scale_factor(&monitor);
        Ok(Self {
            monitor,
            width,
            height,
            scale_factor,
        })
    }

    pub fn primary() -> Result<Self> {
        let monitors = Monitor::all().context("Failed to enumerate monitors")?;
        if monitors.is_empty() {
            anyhow::bail!("No monitors found");
        }
        let primary = monitors
            .iter()
            .find(|m| m.is_primary())
            .cloned()
            .unwrap_or_else(|| monitors[0].clone());
        let width = primary.width();
        let height = primary.height();
        let scale_factor = Self::detect_scale_factor(&primary);
        Ok(Self {
            monitor: primary,
            width,
            height,
            scale_factor,
        })
    }

    fn detect_scale_factor(monitor: &Monitor) -> f64 {
        #[cfg(target_os = "windows")]
        {
            Self::windows_scale_factor(monitor)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = monitor;
            1.0
        }
    }

    #[cfg(target_os = "windows")]
    fn windows_scale_factor(_monitor: &Monitor) -> f64 {
        use windows::Win32::UI::HiDpi::{GetDpiForSystem, MDT_EFFECTIVE_DPI};
        use windows::Win32::Graphics::Gdi::{GetDeviceCaps, LOGPIXELSX, HDC};
        unsafe {
            let hdc = windows::Win32::Graphics::Gdi::GetDC(None);
            if hdc.is_invalid() {
                return 1.0;
            }
            let dpi_x = GetDeviceCaps(Some(hdc), LOGPIXELSX) as f64;
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc);
            let scale = dpi_x / 96.0;
            if scale > 0.0 && scale.is_finite() {
                scale
            } else {
                1.0
            }
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub fn logical_dimensions(&self) -> (f64, f64) {
        (self.width as f64 / self.scale_factor, self.height as f64 / self.scale_factor)
    }

    pub fn capture_frame(&self) -> Result<RgbaImage> {
        let img = self
            .monitor
            .capture_image()
            .context("Failed to capture screen")?;
        let w = img.width();
        let h = img.height();
        let raw = img.into_raw();
        RgbaImage::from_raw(w, h, raw).context("Failed to create RgbaImage from capture buffer")
    }

    pub fn is_blank_frame(frame: &RgbaImage) -> bool {
        let (w, h) = frame.dimensions();
        if w == 0 || h == 0 {
            return true;
        }
        let sample_step = ((w / 8).max(1), (h / 8).max(1));
        let mut non_black = 0u32;
        let mut total = 0u32;
        for y in (0..h).step_by(sample_step.1 as usize) {
            for x in (0..w).step_by(sample_step.0 as usize) {
                let p = frame.get_pixel(x, y);
                if p[0] > 5 || p[1] > 5 || p[2] > 5 {
                    non_black += 1;
                }
                total += 1;
            }
        }
        total > 0 && (non_black as f64 / total as f64) < 0.001
    }

    pub fn capture_interval(&self) -> Duration {
        Duration::from_millis(100)
    }
}
