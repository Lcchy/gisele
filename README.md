A generative midi sequencer running on Jack, controlled via OSC.

### Run on Fedora36

- Install `pipewire-jack-audio-connection-kit-devel` as a dep for building
- Use `helvum` flatpak to route midi in pipewire
- Use `VCV rack` for testing (TODO: switch to pd one day, pipewire shenanigans)

### Goals:

- Percussion slicer:
  - rand note_len is uniform({1/4,1})
- generative random deviation from minimalistic base cell

### Steps:

- Big refactor: Add a note_ref_buffer, that is the one being actually played. Instead of of a thousand conditions SEE (refactor_event_buffer). TODO do not use refs, just duplicate the notes
- Add random deviations from Vec<Event> base: add gen_rand_note() in jack_process
- Give random params like note_len etc a non uniform probability
- Introduce cells/flows: randomness functions that can be used for deviation or base, harm or rythm:
  - euclidean
  - arpeggio
  - polyrythms (is a relationship of rythms?)
  - Retrigs
  - minimalism
  - lfo for osc param control
- Add midi in for randomization seeding
- Add osc output

### TODO:

- add some frames here for precise timing, as a process cycle is 42ms, see jack doc. This should allow to map events on specific frames, making the above if condition redundant - inspi: https://github.com/free-creations/a2jmidi
- If perf is bad: have a stream of events consumed in the jack process, filled by external threads for random deviation generation, based on base sequence stored as Vec<Event>. Use a dynamic stream height, flush when reseeding or so
- LATER: have a central sequencer process that pushes out events to jack midi or osc sender
- Clean up unwraps

### Links:

- Inspirations: 
  - Polyend Play https://www.youtube.com/watch?v=JAQXqoKRfzs
  - Torso T1
  - https://llllllll.co/t/generative-sequencers/19155
  - https://llllllll.co/t/generative-systems/4142/4
  - https://llllllll.co/t/emergence-and-generative-art/2117
  - https://www.youtube.com/watch?v=JPFv3adyLB4
- Midi ref: https://www.cs.cmu.edu/~music/cmsip/readings/davids-midi-spec.htm
- Jack API: https://jackaudio.org/api/group__TimeFunctions.html
