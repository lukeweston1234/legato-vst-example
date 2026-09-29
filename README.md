# Legato VST Example

This is an example showing how you can export a Legato project as a VST/CLAP plugin.

This was made possible by the `nice-plug` crate, which you can read more about [here](https://codeberg.org/RustAudio/nice-plug).

### Building

`cargo nice-plug bundle legato-vst --release`

Copy to your system's VST3 folder, on MacOS mine is found here:

`cp -R target/bundled/legato-vst.vst3 /Library/Audio/Plug-Ins/VST3/`
