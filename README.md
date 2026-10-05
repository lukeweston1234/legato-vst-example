# Legato VST Example

This is an example showing how you can export a Legato project as a VST/CLAP plugin.

This is a small polysynth with reverb.

It demonstrates two of the three ways that users can control Legato applications: the plugin param and message passing.

For audio rate modulation, this is possible with a SPSC using the `external` node in Legato, you can see the examples in 
the upstream Legato repository.

The actual Legato DSL code is located in the `.legato` file in the project root.

### Credit to Nice Plug

This was made possible by the `nice-plug` crate, which you can read more about [here](https://codeberg.org/RustAudio/nice-plug).

### Building

`cargo nice-plug bundle legato-vst --release`

Copy to your system's VST3 folder, on MacOS mine is found here:

`cp -R target/bundled/legato-vst.vst3 /Library/Audio/Plug-Ins/VST3/`

### TODO

This isn't actually nixified, only just a basic dev shell.
