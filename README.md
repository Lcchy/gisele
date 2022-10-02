### Run on Fedora36

- Install `pipewire-jack-audio-connection-kit-devel` as a dep for building
- Use `helvum` flatpak to route midi in pipewire
- Use `VCV rack` for testing (TODO: switch to pd one day, pipewire shenanigans)

### Steps:

- Make osc interface and pd patch for control: reseed and param ctrl
- Start with the goal of the percussion slicer: Introduce generative (live) randomness: random deviation from base harmonic and rythm quantization - split up into base and deviations
- Give random params like note_len etc a non uniform probability
- Maybe going to need a seperate thread for reseeding of EventBuffer? Or keep it in jack thread?
- Introduce cells/flows: randomness functions that can be used for deviation or base, harm or rythm:
  - euclidean
  - arpeggio
  - polyrythms (is a relationship of rythms?)
  - Retrigs
  - minimalism
  - lfo for osc param control
- generative random deviation from minimalistic base cell
- Add midi in for randomization seeding
- Add osc output
- Clean up unwraps

### Links:

- Midi ref: https://www.cs.cmu.edu/~music/cmsip/readings/davids-midi-spec.htm
- Jack API: https://jackaudio.org/api/group__TimeFunctions.html
