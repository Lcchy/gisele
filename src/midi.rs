use anyhow::anyhow;
use rand::Rng;
use rand_distr::{Distribution, Normal, Uniform};
use rust_music_theory::{
    note::{Note, Notes, PitchClass},
    scale::{Direction, Mode, Scale, ScaleType},
};

use crate::{
    seq::{
        BaseSeq, BaseSeqParams,
        BaseSeqType::{Euclid, Random},
        EuclidBase, Event, RandomBase,
    },
    EventType,
};

#[derive(Debug, Copy, Clone)]
pub struct MidiNote {
    pub on_off: bool,
    /// Channel, should be 1-16
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
}

impl MidiNote {
    pub fn get_raw_note_on_bytes(&self) -> [u8; 3] {
        [
            (8 + self.on_off as u8) * 16 + (self.channel - 1),
            self.pitch,
            self.velocity,
        ]
    }
}

pub fn note_to_midi_pitch(note: &Note) -> u8 {
    (note.octave + 1) * 12 + note.pitch_class.into_u8()
}

pub fn midi_pitch_to_note(pitch: u8) -> anyhow::Result<Note> {
    // We only allow midi pitch >= 12 because C_0=12 and rust_music_theory
    // does not allow for negative octaves.
    let octave = (pitch / 12)
        .checked_sub(1)
        .ok_or_else(|| anyhow!("Midi pitch must be >= 12"))?;
    Ok(Note {
        pitch_class: PitchClass::from_u8(pitch),
        octave,
    })
}

pub fn gen_rand_midi_vec(rand_seq: &BaseSeq) -> Vec<Event> {
    let mut rng = rand::thread_rng();
    let mut events_buffer = vec![];

    let params = rand_seq.params.read();
    if let BaseSeqParams {
        ty: Random(RandomBase { nb_events }),
        loop_length,
        root_note,
        note_len_avg,
        note_len_div,
        velocity_avg,
        velocity_div,
        midi_ch,
    } = params.clone()
    {
        // Harmonic quantization
        let scale = Scale::new(
            ScaleType::Diatonic,
            root_note.pitch_class,
            root_note.octave,
            Some(Mode::Ionian),
            Direction::Ascending,
        )
        .unwrap();
        let scale_notes = scale.notes();
        let velocity_distr = Normal::new(velocity_avg as f32, velocity_div).unwrap();
        let note_len_distr = Normal::new(note_len_avg, note_len_div).unwrap();
        let time_incr_distr = Uniform::new(0., loop_length);

        let mut step_offset = 0.;
        for _ in 0..nb_events {
            let pitch = rng.gen_range(0..scale_notes.len());
            let velocity = velocity_distr.sample(&mut rng) as u8;
            let note_len = note_len_distr.sample(&mut rng);

            let event_midi_on = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: midi_ch,
                    pitch: note_to_midi_pitch(&scale_notes[pitch]),
                    velocity,
                    on_off: true,
                }),
                bar_pos: step_offset,
                id: rand_seq.id,
            };
            let event_midi_off = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: midi_ch,
                    pitch: note_to_midi_pitch(&scale_notes[pitch]),
                    velocity,
                    on_off: false,
                }),
                bar_pos: (step_offset + note_len) % loop_length,
                id: rand_seq.id,
            };

            events_buffer.push(event_midi_on);
            events_buffer.push(event_midi_off);
            let time_incr = time_incr_distr.sample(&mut rng);
            step_offset = (step_offset + time_incr) % loop_length;
        }
    } else {
        eprintln!("Could not insert BaseSeq as its not Random.")
    }

    events_buffer
}

/// After http://cgm.cs.mcgill.ca/~godfried/publications/banff.pdf
fn gen_euclid(pulses: u32, steps: u32) -> anyhow::Result<Vec<u8>> {
    if steps < pulses {
        anyhow::bail!("Steps should be less than pulses.")
    }
    let head = vec![vec![1u8]; pulses as usize];
    let tail = vec![vec![0u8]; (steps - pulses) as usize];

    fn gen_euclid_rec(mut head: Vec<Vec<u8>>, mut tail: Vec<Vec<u8>>) -> Vec<u8> {
        let mut new_head = vec![];
        if head.is_empty() {
            return tail.concat();
        }
        while let Some(t) = tail.pop() {
            if let Some(h) = head.pop() {
                new_head.push([h, t].concat());
            } else {
                tail.push(t);
                break;
            }
        }

        if tail.is_empty() && !head.is_empty() {
            tail = head;
        }
        if tail.len() <= 1 {
            return [new_head.concat(), tail.concat()].concat();
        }
        gen_euclid_rec(new_head, tail)
    }

    Ok(gen_euclid_rec(head, tail))
}

pub fn gen_euclid_midi_vec(euclid_seq: &BaseSeq) -> anyhow::Result<Vec<Event>> {
    let mut events_buffer = vec![];

    let params = euclid_seq.params.read();
    if let BaseSeqParams {
        ty: Euclid(EuclidBase { pulses, steps }),
        root_note,
        note_len_avg,
        note_len_div,
        velocity_avg,
        velocity_div,
        midi_ch,
        loop_length,
    } = params.clone()
    {
        if loop_length % steps as f32 != 0. {
            eprintln!("Could not generate euclidean rhythm for indivisible loop-length.");
            return Ok(events_buffer);
        }

        let mut rng = rand::thread_rng();
        let velocity_distr = Normal::new(velocity_avg as f32, velocity_div).unwrap();
        let note_len_distr = Normal::new(note_len_avg, note_len_div).unwrap();

        let euclid_step_len_bar = loop_length / (steps as f32);
        let euclid_rhythm = gen_euclid(pulses, steps)?;

        let pitch = note_to_midi_pitch(&root_note);

        let mut time_offset = 0.;
        for i in euclid_rhythm {
            let velocity = velocity_distr.sample(&mut rng) as u8;
            let note_len = note_len_distr.sample(&mut rng);

            let event_midi_on = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: midi_ch,
                    pitch,
                    velocity,
                    on_off: true,
                }),
                bar_pos: time_offset,
                id: euclid_seq.id,
            };
            let event_midi_off = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: midi_ch,
                    pitch,
                    velocity,
                    on_off: false,
                }),
                bar_pos: (time_offset + note_len) % loop_length,
                id: euclid_seq.id,
            };

            time_offset += euclid_step_len_bar;

            if i == 0 {
                continue;
            }

            events_buffer.push(event_midi_on);
            events_buffer.push(event_midi_off);
        }
    } else {
        eprintln!("Could not insert BaseSeq as its not Euclidean.")
    }
    Ok(events_buffer)
}

#[test]
fn test_euclid() {
    assert_eq!(gen_euclid(0, 0).unwrap(), vec![]);
    assert_eq!(gen_euclid(0, 1).unwrap(), vec![0]);
    assert_eq!(gen_euclid(1, 1).unwrap(), vec![1]);
    assert_eq!(gen_euclid(1, 2).unwrap(), vec![1, 0]);
    assert_eq!(gen_euclid(0, 3).unwrap(), vec![0, 0, 0]);
    assert_eq!(gen_euclid(3, 3).unwrap(), vec![1, 1, 1]);
    assert_eq!(gen_euclid(2, 3).unwrap(), vec![1, 0, 1]);
    assert_eq!(gen_euclid(1, 3).unwrap(), vec![1, 0, 0]);
    assert_eq!(gen_euclid(1, 4).unwrap(), vec![1, 0, 0, 0,]);
    assert_eq!(gen_euclid(3, 4).unwrap(), vec![1, 0, 1, 1]);
    assert_eq!(gen_euclid(2, 5).unwrap(), vec![1, 0, 1, 0, 0]);
    assert_eq!(gen_euclid(5, 8).unwrap(), vec![1, 0, 1, 1, 0, 1, 1, 0]);
    assert_eq!(gen_euclid(7, 8).unwrap(), vec![1, 0, 1, 1, 1, 1, 1, 1]);
    assert_eq!(
        gen_euclid(4, 12).unwrap(),
        vec![1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0,]
    );
    assert_eq!(
        gen_euclid(13, 24).unwrap(),
        vec![1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]
    );
}

#[test]
fn test_note_to_midi_pitch() {
    assert_eq!(
        note_to_midi_pitch(&Note {
            pitch_class: PitchClass::A,
            octave: 4
        }),
        69
    );

    let note = Note {
        pitch_class: PitchClass::A,
        octave: 0,
    };
    eprintln!("{}", note.octave);
    assert_eq!(
        note_to_midi_pitch(&Note {
            pitch_class: PitchClass::A,
            octave: 0
        }),
        21
    );
    assert_eq!(
        note_to_midi_pitch(&Note {
            pitch_class: PitchClass::B,
            octave: 6
        }),
        95
    );
}

#[test]
fn test_midi_pitch_to_note() {
    let a4 = midi_pitch_to_note(69).unwrap();
    assert_eq!(a4.octave, 4);
    assert_eq!(a4.pitch_class, PitchClass::A);

    let a0 = midi_pitch_to_note(21).unwrap();
    assert_eq!(a0.octave, 0);
    assert_eq!(a0.pitch_class, PitchClass::A);

    let b6 = midi_pitch_to_note(95).unwrap();
    assert_eq!(b6.octave, 6);
    assert_eq!(b6.pitch_class, PitchClass::B);
}
