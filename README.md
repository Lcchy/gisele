### Run on Fedora36

- Install `pipewire-jack-audio-connection-kit-devel` as a dep for building
- Use `helvum` flatpak to route midi in pipewire
- Use `VCV rack` for testing (TODO: switch to pd one day, pipewire shenanigans)

### Steps:

- Build a midi randomizer function: 2 fold: pitch, rythm
- Make harmonic randomizer
- Cellularize randomizer with tonal relationships
- Add midi in for randomization seeding
- Add osc output

### Links:

- Midi ref: https://www.cs.cmu.edu/~music/cmsip/readings/davids-midi-spec.htm
- Jack API: https://jackaudio.org/api/group__TimeFunctions.html
