use egui::{Margin, Vec2};
use legato::{
    LegatoApp, LegatoFrontend,
    block_adapter::BlockAdapter,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    msg::{NodeMessage, ParamPayload, RtValue},
    resources::AudioInputKey,
};
use nice_plug::{context::gui::GuiContext, editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, NiceEguiApp, RepaintNotifier,
    create_egui_editor, resizable_window::ResizableWindow, widgets,
};
use std::sync::Arc;

const MIN_WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(300.0, 220.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_WINDOW_SIZE);

/// The fixed block size the Legato graph runs at.
/// Generally we want this lower, we are reporting this much latency
const GRAPH_BLOCK_SIZE: usize = 64;
const CHANNELS: usize = 2;

const MAIN_INPUT: &str = "main";
const REVERB_NODE: &str = "verb";
const MIX_PARAM: &str = "mix";

const GRAPH: &str = include_str!("../.legato");

pub struct ReverbEditor {
    gui_ctx: Option<GuiContext>,
    params: Arc<ReverbParams>,
}

impl NiceEguiApp for ReverbEditor {
    fn build(
        &mut self,
        _egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        self.gui_ctx = Some(nice_gui_ctx);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui_ctx = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(gui_ctx) = self.gui_ctx.as_ref() else {
            return;
        };
        let setter = gui_ctx.param_setter();

        ResizableWindow::new("res-wind")
            .min_size(Vec2::new(MIN_WINDOW_SIZE.width, MIN_WINDOW_SIZE.height))
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.heading("Legato Plate");
                        for param in [
                            &self.params.decay,
                            &self.params.damping,
                            &self.params.bandwidth,
                            &self.params.mix,
                        ] {
                            ui.label(param.name());
                            ui.add(widgets::ParamSlider::for_param(param, &setter));
                        }
                    });
            });
    }
}

// ---------------------------------------------------------------------------------------------------

#[derive(Params)]
pub struct ReverbParams {
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "damping"]
    pub damping: FloatParam,
    #[id = "bandwidth"]
    pub bandwidth: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for ReverbParams {
    fn default() -> Self {
        // Ranges match the clamps inside legato's `Plate480::handle_msg`.
        let unit = |name, default, max| {
            FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max })
                .with_value_to_string(formatters::v2s_f32_rounded(3))
        };
        Self {
            decay: unit("Decay", 0.8, 0.9),
            damping: unit("Damping", 0.3, 0.999),
            bandwidth: unit("Bandwidth", 0.9995, 1.0),
            mix: FloatParam::new("Mix", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

// ---------------------------------------------------------------------------------------------------

/// A snapshot of the plugin params for one host block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub decay: f32,
    pub damping: f32,
    pub bandwidth: f32,
    pub mix: f32,
}

impl ReverbParams {
    pub fn settings(&self) -> Settings {
        Settings {
            decay: self.decay.value(),
            damping: self.damping.value(),
            bandwidth: self.bandwidth.value(),
            mix: self.mix.value(),
        }
    }
}

/// Everything that depends on the sample rate, built in [`Plugin::activate`].
pub struct Engine {
    app: LegatoApp,
    frontend: LegatoFrontend,
    adapter: BlockAdapter,
    input: AudioInputKey,
    /// The last plate settings sent to the graph, so we only send messages on change.
    sent: Option<Settings>,
}

impl Engine {
    pub fn new(sample_rate: usize) -> Self {
        let config = Config {
            sample_rate,
            block_size: GRAPH_BLOCK_SIZE,
            channels: CHANNELS,
            rt_capacity: 0,
        };
        let (app, frontend) = LegatoBuilder::<Unconfigured>::new(config)
            .register_host_audio_input(MAIN_INPUT, CHANNELS)
            .build_dsl(GRAPH)
            .expect("reverb graph should build");
        let adapter = BlockAdapter::new(&app);
        let input = app.audio_input_key(MAIN_INPUT).unwrap();

        Self {
            app,
            frontend,
            adapter,
            input,
            sent: None,
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.adapter.latency_samples()
    }

    /// Forward plugin params to the graph.
    fn sync_params(&mut self, settings: Settings) {
        // `mix` feeds a smoothed `signal` node; this is a lock-free atomic store.
        let _ = self.frontend.set_param(MIX_PARAM, settings.mix);

        // `plate480` has no control inputs yet, so its params are messages, drained once per
        // graph block and applied unsmoothed.
        let sent = self.sent.replace(settings);
        let plate_params = [
            ("decay", settings.decay, sent.map(|s| s.decay)),
            ("damping", settings.damping, sent.map(|s| s.damping)),
            ("bandwidth", settings.bandwidth, sent.map(|s| s.bandwidth)),
        ];
        for (param_name, value, prev) in plate_params {
            if Some(value) != prev {
                let msg = NodeMessage::SetParam(ParamPayload {
                    param_name,
                    value: RtValue::F32(value),
                });
                let _ = self.frontend.send_node_msg(REVERB_NODE, msg);
            }
        }
    }

    pub fn process(&mut self, settings: Settings, io: &mut [&mut [f32]]) {
        self.sync_params(settings);
        self.adapter
            .process(&mut self.app, Some(self.input), io, &[]);
    }

    pub fn reset(&mut self) {
        self.adapter.reset();
    }
}

// ---------------------------------------------------------------------------------------------------

pub struct LegatoReverb {
    params: Arc<ReverbParams>,
    editor_state: Arc<EguiEditorState>,
    engine: Option<Engine>,
}

impl Default for LegatoReverb {
    fn default() -> Self {
        Self {
            params: Arc::new(ReverbParams::default()),
            editor_state: EguiEditorState::from_size(MIN_WINDOW_SIZE, 1.0),
            engine: None,
        }
    }
}

impl Plugin for LegatoReverb {
    const NAME: &'static str = "Legato Plate";
    const VENDOR: &'static str = "Legato";
    const URL: &'static str = "https://github.com/legato-dsp/legato";
    const EMAIL: &'static str = "info@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(CHANNELS as u32),
        main_output_channels: NonZeroU32::new(CHANNELS as u32),
        ..AudioIOLayout::const_default()
    }];

    type Editor = EguiEditor<ReverbEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        create_egui_editor(
            self.editor_state.clone(),
            RepaintNotifier::new(),
            EguiNiceSettings::new().with_resize_hint(RESIZE_HINT),
            ReverbEditor {
                gui_ctx: None,
                params: self.params.clone(),
            },
        )
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        let engine = Engine::new(buffer_config.sample_rate as usize);
        context.set_latency_samples(engine.latency_samples() as u32);
        self.engine = Some(engine);
        true
    }

    fn reset(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        if let Some(engine) = &mut self.engine {
            engine.process(self.params.settings(), buffer.as_slice());
        }
        // Keep processing through silence so the tail rings out.
        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for LegatoReverb {
    const CLAP_ID: &'static str = "dev.legato.plate";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A Legato plate reverb");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Reverb,
    ];
}

impl Vst3Plugin for LegatoReverb {
    const VST3_CLASS_ID: [u8; 16] = *b"LegatoPlateVerb!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Reverb];
}

nice_export_clap!(LegatoReverb);
nice_export_vst3!(LegatoReverb);

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mix: f32) -> Settings {
        Settings {
            mix,
            ..ReverbParams::default().settings()
        }
    }

    /// Runs `frames` of `input` (the same on both channels) through the engine in one call.
    fn render(engine: &mut Engine, settings: Settings, input: &[f32]) -> [Vec<f32>; 2] {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        engine.process(settings, &mut [&mut l, &mut r]);
        [l, r]
    }

    #[test]
    fn fully_dry_mix_is_a_delayed_passthrough() {
        let mut engine = Engine::new(48_000);
        // Let the smoothed mix signal settle at 0 before measuring.
        render(&mut engine, settings(0.0), &[0.0; 48_000]);

        let mut impulse = vec![0.0; 1024];
        impulse[0] = 1.0;
        let latency = engine.latency_samples();
        for chan in render(&mut engine, settings(0.0), &impulse) {
            for (i, s) in chan.iter().enumerate() {
                let expected = if i == latency { 1.0 } else { 0.0 };
                assert!((s - expected).abs() < 1e-4, "sample {i}: {s}");
            }
        }
    }

    #[test]
    fn mix_changes_are_smoothed() {
        let mut engine = Engine::new(48_000);
        render(&mut engine, settings(0.0), &[1.0; 48_000]);

        // Jumping to fully wet should fade the dry signal out, not cut it on the next block.
        let [l, _] = render(&mut engine, settings(1.0), &[1.0; 512]);
        let after_step = &l[engine.latency_samples()..];
        assert!(
            after_step[0] > 0.9,
            "dry cut off instantly: {}",
            after_step[0]
        );
        assert!(after_step.windows(2).all(|w| (w[1] - w[0]).abs() < 0.01));
    }

    #[test]
    fn impulse_produces_a_delayed_tail_under_odd_host_blocks() {
        let mut engine = Engine::new(48_000);
        let settings = ReverbParams::default().settings();
        let mut energy = 0.0;
        let mut first_nonzero = None;
        let mut at = 0;

        for n in [1, 37, 128, 511, 64, 300].into_iter().cycle().take(200) {
            let mut l = vec![0.0; n];
            let mut r = vec![0.0; n];
            if at == 0 {
                l[0] = 1.0;
                r[0] = 1.0;
            }
            engine.process(settings, &mut [&mut l, &mut r]);
            for (i, s) in l.iter().chain(&r).enumerate() {
                assert!(s.is_finite());
                if *s != 0.0 && first_nonzero.is_none() {
                    first_nonzero = Some(at + i % n);
                }
                energy += s * s;
            }
            at += n;
        }

        assert_eq!(first_nonzero, Some(engine.latency_samples()));
        assert!(energy > 1e-3, "reverb tail was silent");
    }
}
