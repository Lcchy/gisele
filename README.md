WIP A generative midi sequencer running on Jack, controlled via OSC.

### Run on Fedora36

- Install `pipewire-jack-audio-connection-kit-devel` as a dep for building
- Use `helvum` flatpak to route midi in pipewire
- Use `Qsynth` for sound testing (pd midi not working with pw)
- Use tmux for live debuging:
  - Split panes: left-right: ctrl+b + % OR top-bottom: ctrl+b + "
  - focus: ctrl+b + arrow
  - `$ tty`, copy your error pane's device file
  - `$ cargo run 2>/dev/pts/N`

### General Structure:

A Sequencer Line:

```
Fixed base sequence (euclidian, minimalism, random, counterpoint, etc..)

-(into)-> Seq Fx (harmonic inversion, pitschift, retrigs, etc...)

-(into)-> Randomization cells (gauss, poison.., markov, genetic mutation, L systems, game of life, ...)

--> output
```

These sequencer lines work in parallel.

2 Models:

1. Dominated:
   One sequencer line (which could be silent in sound) influences parameters of other lines. Composition.
2. Federated:
   The sequencer lines influence each others parameters creating feedback loops, maintaining the balance. Generative.

(Model 1 could control groups of model 2 structures.)

Additionally: choose the correct parameter spaces for live user input.

---

### Intermediate Goals:

- Percussion slicer with Blackbox: use euclids
- Minimalism

### TODO:

- Fix empty doesnt turn notes off: set to stop, wait, delete notes
- Fix: Set loop len to 1, then to 16, gives a silent loop
- Have a stream of events consumed in the jack process, filled by an external thread for random deviation generation, based on base sequence (could be used for e.g. euclidian rhythm, as loop_len could be factored into each BaseSeq). Use a dynamic stream height depending on bpm, flush on param change. Or use a crossbeam::SeqQueue?

- use frames for precise timing, as a process cycle is 42ms, see jack doc. This should allow to map events on specific frames - inspi(see also links): https://github.com/free-creations/a2jmidi
- Use 2 event bufffers: note on and not off? Does it comply with LFO vars Events for example? Would make Pause/Stop and regen_base_seq event_head asjustment easier (+ set note len wouldnt need a sort) -> Do it only in conjunction with a refactor of the buffer logic
- If osc-in processing is too slow, spawn a thread per received msg, or use thread pools
- factorize main jack event loop into structs for clarity, see it as a sliding window with a semi-synced peek. This is the only way we maintain low complexity
- For OSC-out use a thread-pool of osc_senders channels to which we offload from the jack_process.
- Clean up unwraps and [idx]
- Optimize sync_event_head: set to event in curr jack window if we know the cycle to be just about to play | Ambitious and secondary

### Steps:

- Set init vals in control.pd, make it more usable (see add_base_seq)
- Test euclid gen midi with constant note: make it work with general loop_len first, awaits major refactor
- Add random deviations (deviation cells) from BaseSeq: add gen_rand_note() in jack_process
- SLICER
- Improve set_nb_events: seq should be identical when coming back to init_nb
- Add remove_base_seq
- Introduce cells: randomness functions that can be used for deviation or base, harm or rhythm:

  - Overlapping/non-overlapping events (on same channel or not)
  - Chords
  - arpeggio
  - polyrhythms (is a relationship of rhythms?)
  - Retrigs
  - grooves: euclidian, minimalism, amapiano, contre-temps, kontrapunkt, dnb, acid techno, dub
  - lfo for osc param control

- Make params sequencable
- Add harmonic coherent transpose + inversions
- Make smooth BaseSeq transitions possible, using rand walks e.g.
- Add midi in for randomization seeding
- Add midi rec for base seq
- Add manual loop shortening
- Add osc output
- Look into RT priority
  (- Big refactor: Add a note_ref_buffer, that is the one being actually played. Instead of of a thousand conditions SEE (refactor_event_buffer). TODO do not use refs, just duplicate the notes)

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
