//! Windows backend: winit event loop on its own thread + wgpu (DX12 / DirectComposition).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::{EventLoopBuilderExtWindows, WindowAttributesExtWindows};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId, WindowLevel};

use crate::{Error, MonitorInfo, MonitorSelection, OverlayConfig, Result};

pub(crate) enum Cmd {
    Palette(Vec<[u8; 3]>),
    Playing(bool),
    Energy(f32),
    Config(Box<OverlayConfig>),
    Shutdown,
}

pub(crate) struct Handle {
    proxy: EventLoopProxy<Cmd>,
    monitors: Arc<Mutex<Vec<MonitorInfo>>>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub(crate) fn spawn(config: OverlayConfig) -> Result<Self> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let monitors = Arc::new(Mutex::new(Vec::new()));
        let shared = monitors.clone();
        let thread = std::thread::Builder::new()
            .name("glowtune-overlay".into())
            .spawn(move || {
                let event_loop = match EventLoop::<Cmd>::with_user_event()
                    .with_any_thread(true)
                    .build()
                {
                    Ok(el) => el,
                    Err(e) => {
                        let _ = ready_tx.send(Err(Error::Window(e.to_string())));
                        return;
                    }
                };
                let gfx = match Gfx::new() {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(event_loop.create_proxy()));
                event_loop.set_control_flow(ControlFlow::Wait);
                let mut app = App::new(gfx, config, shared);
                if let Err(e) = event_loop.run_app(&mut app) {
                    tracing::error!("overlay event loop failed: {e}");
                }
            })
            .map_err(|e| Error::Thread(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(proxy)) => Ok(Self {
                proxy,
                monitors,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(Error::Thread("overlay thread exited during startup".into())),
        }
    }

    pub(crate) fn send(&self, cmd: Cmd) {
        let _ = self.proxy.send_event(cmd);
    }

    pub(crate) fn monitors(&self) -> Vec<MonitorInfo> {
        self.monitors.lock().map(|m| m.clone()).unwrap_or_default()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.proxy.send_event(Cmd::Shutdown).is_ok()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

// ───────────────────────────── GPU ─────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4],
    screen: [f32; 4],
    anim: [f32; 4],
    mixing: [f32; 4],
    effects: [f32; 4],
    particles: [f32; 4],
    from_colors: [[f32; 4]; 5],
    to_colors: [[f32; 4]; 5],
}

struct Gfx {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    /// Built lazily once the first surface tells us its format.
    pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
}

impl Gfx {
    fn new() -> Result<Self> {
        // DX12 + DirectComposition is the only path with per-pixel transparency
        // here (Vulkan swapchains on Windows only offer Opaque).
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        let instance = wgpu::Instance::new(desc);

        // Render on the GPU that drives the displays (avoids cross-GPU copies).
        // GLOWTUNE_GPU=low forces the integrated GPU, for measuring.
        let power_preference = match std::env::var("GLOWTUNE_GPU").as_deref() {
            Ok("low") => wgpu::PowerPreference::LowPower,
            _ => wgpu::PowerPreference::HighPerformance,
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| Error::Gpu(e.to_string()))?;
        let info = adapter.get_info();
        tracing::info!("overlay GPU: {} ({:?})", info.name, info.backend);

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("glowtune-overlay"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| Error::Gpu(e.to_string()))?;

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glow-uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glow.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glow.wgsl").into()),
        });
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            layout,
            shader,
            pipeline: None,
        })
    }

    fn pipeline(&mut self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        if self.pipeline.as_ref().is_none_or(|(f, _)| *f != format) {
            let layout = self
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("glow"),
                    bind_group_layouts: &[Some(&self.layout)],
                    immediate_size: 0,
                });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("glow"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &self.shader,
                        entry_point: Some("vs"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &self.shader,
                        entry_point: Some("fs"),
                        // No blending: the shader writes final premultiplied pixels.
                        targets: &[Some(format.into())],
                        compilation_options: Default::default(),
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
            self.pipeline = Some((format, pipeline));
        }
        &self.pipeline.as_ref().expect("pipeline just built").1
    }
}

// ───────────────────────────── Strips ─────────────────────────────

#[derive(Clone, Copy, Debug)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

struct Strip {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Strip rectangle in monitor space (physical px).
    rect: [f32; 4],
}

struct MonitorGlow {
    info: MonitorInfo,
    strips: Vec<Strip>,
    thickness_px: f32,
    radius_px: f32,
    /// A fullscreen app or game is in front on this screen: don't draw.
    covered: bool,
}

fn monitor_info(
    m: &winit::monitor::MonitorHandle,
    primary: Option<&winit::monitor::MonitorHandle>,
) -> MonitorInfo {
    let pos = m.position();
    let size = m.size();
    MonitorInfo {
        name: m.name().unwrap_or_default(),
        primary: primary.is_some_and(|p| p == m),
        position: (pos.x, pos.y),
        size: (size.width, size.height),
        scale: m.scale_factor(),
    }
}

/// Strip depth must hold every formation (the shader caps depth at × 1.51).
fn strip_depth(thickness_px: f32) -> u32 {
    (thickness_px * 1.52).ceil() as u32 + 2
}

fn strip_rects(size: (u32, u32), depth: u32) -> Vec<(Edge, [u32; 4])> {
    let (w, h) = size;
    let d = depth.min(h / 2).min(w / 2);
    vec![
        (Edge::Top, [0, 0, w, d]),
        (Edge::Bottom, [0, h - d, w, d]),
        (Edge::Left, [0, d, d, h - 2 * d]),
        (Edge::Right, [w - d, d, d, h - 2 * d]),
    ]
}

fn create_strip(
    event_loop: &ActiveEventLoop,
    gfx: &mut Gfx,
    monitor: &MonitorInfo,
    edge: Edge,
    [x, y, w, h]: [u32; 4],
) -> Result<Strip> {
    let attrs = Window::default_attributes()
        .with_title("Glowtune glow")
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_position(PhysicalPosition::new(
            monitor.position.0 + x as i32,
            monitor.position.1 + y as i32,
        ))
        .with_inner_size(PhysicalSize::new(w, h))
        .with_skip_taskbar(true)
        .with_no_redirection_bitmap(true)
        .with_undecorated_shadow(false)
        .with_drag_and_drop(false)
        .with_class_name("GlowtuneGlowStrip");
    let window = Arc::new(
        event_loop
            .create_window(attrs)
            .map_err(|e| Error::Window(e.to_string()))?,
    );
    window
        .set_cursor_hittest(false)
        .map_err(|e| Error::Window(e.to_string()))?;
    hide_from_switchers(&window);

    let surface = gfx
        .instance
        .create_surface(window.clone())
        .map_err(|e| Error::Gpu(e.to_string()))?;
    let caps = surface.get_capabilities(&gfx.adapter);
    // Non-sRGB 8-bit format: the shader writes gamma-space premultiplied
    // values itself. (sRGB formats would encode color but not alpha.)
    let format = [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ]
    .into_iter()
    .find(|f| caps.formats.contains(f))
    .ok_or_else(|| {
        Error::Gpu(format!(
            "no 8-bit unorm surface format in {:?}",
            caps.formats
        ))
    })?;
    if !caps
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
    {
        return Err(Error::Gpu(format!(
            "no transparent surface support ({:?})",
            caps.alpha_modes
        )));
    }
    // Mailbox/Immediate never block, so eight windows can present in one frame.
    let present_mode = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
        .into_iter()
        .find(|m| caps.present_modes.contains(m))
        .unwrap_or(wgpu::PresentMode::Fifo);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        color_space: Default::default(),
        width: w.max(1),
        height: h.max(1),
        present_mode,
        desired_maximum_frame_latency: 1,
        alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
        view_formats: vec![],
    };
    surface.configure(&gfx.device, &config);
    gfx.pipeline(format);

    let uniforms = gfx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glow-uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = gfx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glow-uniforms"),
        layout: &gfx.layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        }],
    });
    tracing::debug!("strip {edge:?} on {} at {x},{y} {w}x{h}", monitor.name);
    Ok(Strip {
        window,
        surface,
        config,
        uniforms,
        bind_group,
        rect: [x as f32, y as f32, w as f32, h as f32],
    })
}

fn hwnd_of(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return None;
    };
    Some(windows::Win32::Foundation::HWND(h.hwnd.get() as *mut _))
}

/// winit's skip_taskbar only hides the taskbar button. Tool windows are also
/// left out of Alt-Tab and Task View; NOACTIVATE means they never take focus.
fn hide_from_switchers(window: &Window) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };
    let Some(hwnd) = hwnd_of(window) else { return };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = (ex | WS_EX_TOOLWINDOW.0 as isize | WS_EX_NOACTIVATE.0 as isize)
            & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

/// Shows/hides without going through winit: winit recomputes (and resets) the
/// extended styles on every visibility change, which would undo
/// `hide_from_switchers`. SW_SHOWNOACTIVATE never steals focus.
fn show_strip(window: &Window, show: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow};
    let Some(hwnd) = hwnd_of(window) else { return };
    unsafe {
        let _ = ShowWindow(hwnd, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

/// Top-left of the monitor where the foreground window covers the whole
/// screen (a borderless game, fullscreen video...), if any. The desktop and
/// taskbar don't count.
pub(crate) fn fullscreen_monitor_origin() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
        GetWindowThreadProcessId,
    };
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == GetDesktopWindow() || fg == GetShellWindow() {
            return None;
        }
        // Our own full-screen desktop scene is not a game.
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(fg, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(fg, &mut r).ok()?;
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let m = info.rcMonitor;
        let covers =
            r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
        covers.then_some((m.left, m.top))
    }
}

// ───────────────────────────── Animation state ─────────────────────────────

const FADE_IN_TAU: f32 = 0.22;
const FADE_OUT_TAU: f32 = 0.5;
const PALETTE_CROSSFADE: Duration = Duration::from_millis(1600);
const MONITOR_RECHECK: Duration = Duration::from_secs(2);
/// How often to check for a fullscreen app while the glow is visible.
const FULLSCREEN_RECHECK: Duration = Duration::from_millis(500);

/// After this long without light, the GPU device is released entirely (the
/// next play recreates it behind the fade-in). `GLOWTUNE_DEEP_SLEEP_SECS` overrides.
fn deep_sleep_after() -> Duration {
    let secs = std::env::var("GLOWTUNE_DEEP_SLEEP_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(45);
    Duration::from_secs(secs)
}

fn to_linear(c: [u8; 3]) -> [f32; 4] {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(c[0]), f(c[1]), f(c[2]), 1.0]
}

#[derive(Clone)]
struct Palette {
    colors: [[f32; 4]; 5],
    count: usize,
}

impl Palette {
    fn new(srgb: &[[u8; 3]]) -> Self {
        let mut colors = [[0.0; 4]; 5];
        let srgb: Vec<[u8; 3]> = if srgb.is_empty() {
            vec![[139, 124, 255], [92, 160, 255]]
        } else {
            srgb.to_vec()
        };
        for (slot, c) in colors.iter_mut().zip(&srgb) {
            *slot = to_linear(*c);
        }
        Self {
            colors,
            count: srgb.len().min(5),
        }
    }
}

struct App {
    /// `None` while in deep sleep (GPU fully released).
    gfx: Option<Gfx>,
    /// When the glow last went dark; drives the deep-sleep timer.
    dark_since: Option<Instant>,
    config: OverlayConfig,
    shared_monitors: Arc<Mutex<Vec<MonitorInfo>>>,
    monitors: Vec<MonitorGlow>,
    monitors_checked: Option<Instant>,
    fullscreen_checked: Option<Instant>,

    start: Instant,
    last_tick: Instant,
    next_frame: Instant,
    playing: bool,
    fade: f32,
    energy: f32,
    target_energy: f32,
    drift: f32,
    from: Palette,
    to: Palette,
    crossfade_start: Instant,
    shown: bool,
}

impl App {
    fn new(gfx: Gfx, config: OverlayConfig, shared_monitors: Arc<Mutex<Vec<MonitorInfo>>>) -> Self {
        let now = Instant::now();
        let fallback = Palette::new(&[]);
        Self {
            gfx: Some(gfx),
            dark_since: Some(now),
            config,
            shared_monitors,
            monitors: Vec::new(),
            monitors_checked: None,
            fullscreen_checked: None,
            start: now,
            last_tick: now,
            next_frame: now,
            playing: false,
            fade: 0.0,
            energy: 0.0,
            target_energy: 0.0,
            drift: 0.0,
            from: fallback.clone(),
            to: fallback,
            crossfade_start: now - PALETTE_CROSSFADE,
            shown: false,
        }
    }

    fn wants_light(&self) -> bool {
        self.playing && self.config.enabled
    }

    fn animating(&self) -> bool {
        self.wants_light() || self.fade > 0.0
    }

    /// (Re)builds the strip windows if the monitor layout or glow size changed.
    fn sync_monitors(&mut self, event_loop: &ActiveEventLoop) {
        self.monitors_checked = Some(Instant::now());
        let primary = event_loop.primary_monitor();
        let all: Vec<MonitorInfo> = event_loop
            .available_monitors()
            .map(|m| monitor_info(&m, primary.as_ref()))
            .collect();
        if let Ok(mut shared) = self.shared_monitors.lock() {
            shared.clone_from(&all);
        }

        let wanted: Vec<&MonitorInfo> = all
            .iter()
            .filter(|m| match &self.config.monitors {
                MonitorSelection::All => true,
                MonitorSelection::Primary => m.primary,
                MonitorSelection::Only(names) => names.contains(&m.name),
            })
            .collect();

        let same = wanted.len() == self.monitors.len()
            && wanted.iter().zip(&self.monitors).all(|(w, have)| {
                **w == have.info
                    && have.thickness_px == self.config.thickness * w.scale as f32
                    && have.radius_px == self.config.corner_radius * w.scale as f32
            });
        if same {
            return;
        }

        tracing::info!("building glow for {} monitor(s)", wanted.len());
        self.fullscreen_checked = None; // re-check before the new strips show
        self.monitors.clear(); // drops old windows + surfaces
        let Some(gfx) = self.gfx.as_mut() else { return };
        for info in wanted {
            let thickness_px = self.config.thickness * info.scale as f32;
            let radius_px = self.config.corner_radius * info.scale as f32;
            let mut strips = Vec::with_capacity(4);
            for (edge, rect) in strip_rects(info.size, strip_depth(thickness_px)) {
                match create_strip(event_loop, gfx, info, edge, rect) {
                    Ok(s) => strips.push(s),
                    Err(e) => tracing::error!("glow strip {edge:?} on {}: {e}", info.name),
                }
            }
            self.monitors.push(MonitorGlow {
                info: info.clone(),
                strips,
                thickness_px,
                radius_px,
                covered: false,
            });
        }
        self.shown = false;
    }

    fn set_shown(&mut self, shown: bool) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        for m in &self.monitors {
            for s in &m.strips {
                show_strip(&s.window, shown && !m.covered);
            }
        }
    }

    /// Hides the glow on screens where a fullscreen app or game is in front.
    fn update_fullscreen(&mut self) {
        self.fullscreen_checked = Some(Instant::now());
        let origin = if self.config.hide_on_fullscreen {
            fullscreen_monitor_origin()
        } else {
            None
        };
        for m in &mut self.monitors {
            let covered = origin == Some(m.info.position);
            if covered != m.covered {
                m.covered = covered;
                tracing::debug!(
                    "glow on {}: {}",
                    m.info.name,
                    if covered {
                        "hidden (fullscreen app)"
                    } else {
                        "shown"
                    }
                );
                if self.shown {
                    for s in &m.strips {
                        show_strip(&s.window, !covered);
                    }
                }
            }
        }
    }

    fn tick(&mut self, now: Instant) {
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;

        let target = if self.wants_light() { 1.0 } else { 0.0 };
        let tau = if target > self.fade {
            FADE_IN_TAU
        } else {
            FADE_OUT_TAU
        };
        self.fade += (target - self.fade) * (1.0 - (-dt / tau).exp());
        if target == 0.0 && self.fade < 0.003 {
            self.fade = 0.0;
        }

        // Beat energy: fast attack, slow release (Phase 3 feeds this).
        let e_tau = if self.target_energy > self.energy {
            0.04
        } else {
            0.25
        };
        self.energy += (self.target_energy - self.energy) * (1.0 - (-dt / e_tau).exp());

        self.drift = (self.drift + dt * self.config.drift_speed / 60.0).fract();
    }

    fn render(&mut self, now: Instant) {
        let Some(gfx) = self.gfx.as_ref() else { return };
        let time = now.duration_since(self.start).as_secs_f32();
        let mix = (now.duration_since(self.crossfade_start).as_secs_f32()
            / PALETTE_CROSSFADE.as_secs_f32())
        .min(1.0);
        let energy = self.energy * self.config.beat_reactivity;
        let mut lost = false;

        for m in self.monitors.iter().filter(|m| !m.covered) {
            for s in &m.strips {
                let u = Uniforms {
                    rect: s.rect,
                    screen: [
                        m.info.size.0 as f32,
                        m.info.size.1 as f32,
                        m.thickness_px,
                        m.radius_px,
                    ],
                    anim: [time, self.fade, energy, self.config.intensity],
                    mixing: [
                        mix,
                        self.from.count as f32,
                        self.to.count as f32,
                        self.drift,
                    ],
                    effects: [
                        self.config.effect as u32 as f32,
                        self.config.effect_speed
                            * if self.config.reverse_motion {
                                -1.0
                            } else {
                                1.0
                            },
                        self.config.effect_strength,
                        self.config.particle_sparkle,
                    ],
                    particles: [
                        self.config.particles as u32 as f32,
                        self.config.particle_density,
                        2.0 * self.config.particle_size * m.info.scale as f32,
                        self.config.particle_speed,
                    ],
                    from_colors: self.from.colors,
                    to_colors: self.to.colors,
                };
                gfx.queue
                    .write_buffer(&s.uniforms, 0, bytemuck::bytes_of(&u));

                let frame = match s.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(f)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        s.surface.configure(&gfx.device, &s.config);
                        lost = true;
                        continue;
                    }
                    other => {
                        tracing::debug!("skipping frame: {other:?}");
                        continue;
                    }
                };
                let view = frame.texture.create_view(&Default::default());
                let mut enc = gfx.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("glow"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    if let Some((_, pipeline)) = &gfx.pipeline {
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &s.bind_group, &[]);
                        pass.draw(0..3, 0..1);
                    }
                }
                gfx.queue.submit([enc.finish()]);
                gfx.queue.present(frame);
            }
        }
        if lost {
            // A display change usually caused this; re-check the layout soon.
            self.monitors_checked = None;
        }
    }

    fn handle(&mut self, event_loop: &ActiveEventLoop, cmd: Cmd) {
        match cmd {
            Cmd::Palette(colors) => {
                let now = Instant::now();
                // Start from whatever is on screen right now (mid-crossfade safe).
                let mid = (now.duration_since(self.crossfade_start).as_secs_f32()
                    / PALETTE_CROSSFADE.as_secs_f32())
                .min(1.0);
                if mid >= 0.5 {
                    self.from = self.to.clone();
                }
                self.to = Palette::new(&colors);
                // If the glow is hidden, switch instantly.
                self.crossfade_start = if self.fade == 0.0 {
                    now - PALETTE_CROSSFADE
                } else {
                    now
                };
            }
            Cmd::Playing(p) => self.playing = p,
            Cmd::Energy(e) => {
                self.target_energy = if e.is_finite() {
                    e.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            }
            Cmd::Config(c) => {
                self.config = *c;
                self.monitors_checked = None;
                if self.shown {
                    self.sync_monitors(event_loop);
                }
            }
            Cmd::Shutdown => event_loop.exit(),
        }
    }
}

impl ApplicationHandler<Cmd> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Record monitors for the settings UI; windows are built on first play.
        let primary = event_loop.primary_monitor();
        if let Ok(mut shared) = self.shared_monitors.lock() {
            *shared = event_loop
                .available_monitors()
                .map(|m| monitor_info(&m, primary.as_ref()))
                .collect();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, cmd: Cmd) {
        self.handle(event_loop, cmd);
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let WindowEvent::ScaleFactorChanged { .. } = event {
            self.monitors_checked = None;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if !self.animating() {
            self.set_shown(false);
            let dark_since = *self.dark_since.get_or_insert(now);
            let release_at = dark_since + deep_sleep_after();
            if self.gfx.is_some() && now >= release_at {
                // Deep sleep: drop windows, swapchains and the GPU device.
                tracing::info!("glow idle: releasing GPU");
                self.monitors.clear();
                self.gfx = None;
            }
            event_loop.set_control_flow(if self.gfx.is_some() {
                ControlFlow::WaitUntil(release_at) // one wake-up to release, then sleep
            } else {
                ControlFlow::Wait // sleep until a command arrives
            });
            return;
        }
        self.dark_since = None;

        if self.gfx.is_none() {
            match Gfx::new() {
                Ok(gfx) => {
                    self.gfx = Some(gfx);
                    self.monitors_checked = None;
                }
                Err(e) => {
                    tracing::error!("glow GPU unavailable: {e}");
                    self.playing = false;
                    self.fade = 0.0;
                    return;
                }
            }
        }

        if self
            .monitors_checked
            .is_none_or(|t| now.duration_since(t) > MONITOR_RECHECK)
        {
            self.sync_monitors(event_loop);
        }
        if self
            .fullscreen_checked
            .is_none_or(|t| now.duration_since(t) > FULLSCREEN_RECHECK)
        {
            self.update_fullscreen();
        }
        if !self.shown {
            self.last_tick = now;
            self.next_frame = now;
            // Draw the first (fully faded) frame before showing, so windows
            // never flash with stale content.
            self.render(now);
            self.set_shown(true);
        }

        if now >= self.next_frame {
            self.tick(now);
            self.render(now);
            let frame = Duration::from_secs_f64(1.0 / self.config.max_fps.clamp(10, 240) as f64);
            self.next_frame += frame;
            if self.next_frame < now {
                self.next_frame = now + frame; // fell behind; don't try to catch up
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tile_the_border_without_overlap() {
        let rects = strip_rects((2560, 1440), 70);
        let area: u64 = rects.iter().map(|(_, r)| r[2] as u64 * r[3] as u64).sum();
        let border = 2560u64 * 1440 - (2560 - 140) as u64 * (1440 - 140) as u64;
        assert_eq!(area, border);
    }

    #[test]
    fn strip_depth_covers_full_swell() {
        // All spatial formations clamp their full beat swell to × 1.51.
        let t = 56.0 * 1.5;
        assert!(strip_depth(t) as f32 >= t * 1.51);
    }

    #[test]
    #[ignore = "requires a Windows DX12 GPU"]
    fn glow_pipeline_compiles_on_gpu() {
        let mut gfx = Gfx::new().expect("DX12 device");
        gfx.pipeline(wgpu::TextureFormat::Bgra8Unorm);
    }

    #[test]
    fn glow_shader_validates() {
        let shader = wgpu::naga::front::wgsl::parse_str(include_str!("glow.wgsl"))
            .expect("valid glow shader syntax");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&shader)
        .expect("valid glow shader types and bindings");
    }

    #[test]
    fn uniforms_match_wgsl_layout() {
        // 6 vec4 + 2 × array<vec4, 5> = 16 × 16 bytes.
        assert_eq!(std::mem::size_of::<Uniforms>(), 16 * 16);
    }
}
