WIP A generative midi sequencer running on Jack, controlled via OSC.

### Run on Fedora36

- Install `pipewire-jack-audio-connection-kit-devel` as a dep for building
- Use `helvum` flatpak to route midi in pipewire
- Use `Qsynth` for sound testing (pd midi not working with pw)

### General Structure:

A Sequencer Line:

```
Fixed base seqs (euclidian, minimalism, dub, random, counterpoint, etc..)

-(into)-> Seq Fx (harmonic inversion, pitschift, retrigs, etc...)

-(into)-> Randomization cells (gauss, poison.., genetic mutation, L systems, game of life, ...)

--> output
```

These sequencer lines work in parallell.

2 Models:

1. Dominated:
   One sequencer line (which could be silent in sound) influences parameters of other lines. Composition.
2. Federated:
   The sequencer lines influence each others parameters creating feedback loops, maintaining the balance. Generative.

(Model 1 could control groups of model 2 structures.)

Additionally: choose the correct parameters for live user input.

---

### Intermediate Goals:

- Percussion slicer:
  - rand note_len is uniform({1/4,1})
- generative random deviation from minimalistic base cell

### Steps:

(- Big refactor: Add a note_ref_buffer, that is the one being actually played. Instead of of a thousand conditions SEE (refactor_event_buffer). TODO do not use refs, just duplicate the notes)

- Test out Gisele with Blackbox for slicer rythm:
  - Add euclid gen midi with constant note
  - Add random deviations from Vec<Event> base: add gen_rand_note() in jack_process, add LFOS for modulation of note center
- Fix Set nb events freezes loop until stop start --> Fix reseed in fact
- Fix/test set loop length bars
- Add velocity param
- Non uniform probs for seq gen params
- Introduce cells/flows: randomness functions that can be used for deviation or base, harm or rythm:

  - Overlapping/non-overlapping events (on same channel or not)
  - Chords
  - arpeggio
  - polyrythms (is a relationship of rythms?)
  - Retrigs
  - grooves: euclidian, minimalism, amapiano, contre-temps, kontrapunkt, dnb, acid techno, dub
  - lfo for osc param control

- Make params sequencable
- Add harmonic coherent transpose + iversions
- Add midi in for randomization seeding
- Add manual loop shortening
- Add osc output
- Look into RT priority

### TODO:

- add some frames here for precise timing, as a process cycle is 42ms, see jack doc. This should allow to map events on specific frames, making the above if condition redundant - inspi: https://github.com/free-creations/a2jmidi
- If perf is bad: have a stream of events consumed in the jack process, filled by external threads for random deviation generation, based on base sequence stored as Vec<Event>. Use a dynamic stream height, flush when reseeding or so
- LATER: have a central sequencer process that pushes out events to jack midi or osc sender
- Clean up unwraps

### Links:

- Euclid implem: http://cgm.cs.mcgill.ca/~godfried/publications/banff.pdf
- Inspirations:
  - Polyend Play https://www.youtube.com/watch?v=JAQXqoKRfzs
  - Torso T1
  - https://llllllll.co/t/generative-sequencers/19155
  - https://llllllll.co/t/generative-systems/4142/4
  - https://llllllll.co/t/emergence-and-generative-art/2117
  - https://www.youtube.com/watch?v=JPFv3adyLB4
- Midi ref: https://www.cs.cmu.edu/~music/cmsip/readings/davids-midi-spec.htm
- Jack API: https://jackaudio.org/api/group__TimeFunctions.html
- Refs for jack frame event sync:
   - https://github.com/free-creations/a2jmidi
   - https://github.com/jackaudio/tools/blob/master/connect.c
