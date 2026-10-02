# Legato VST Example

This is an example showing how you can export a Legato project as a VST/CLAP plugin.

It's a small instrument: an 8-voice polyphonic sawtooth (`poly_voice` → saw + ADSR per voice), into a
resonant lowpass, into a plate reverb. The whole graph lives in [`.legato`](.legato); `src/lib.rs` wires
plugin params to the graph's `signal` nodes and forwards host note events through the `BlockAdapter`,
which keeps MIDI sample-aligned with the audio.

This was made possible by the `nice-plug` crate, which you can read more about [here](https://codeberg.org/RustAudio/nice-plug).

### Building

`cargo nice-plug bundle legato-vst --release`

Copy to your system's VST3 folder, on MacOS mine is found here:

`cp -R target/bundled/legato-vst.vst3 /Library/Audio/Plug-Ins/VST3/`
