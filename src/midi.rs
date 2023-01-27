use rand::Rng;
use rust_music_theory::{
    note::{Note, Notes, PitchClass},
    scale::{Direction, Mode, Scale, ScaleType},
};

use crate::{
    seq::{
        BaseSeq,
        BaseSeqParams::{Euclid, Random},
        EuclidBase, Event, RandomBase, SeqParams,
    },
    EventType,
};

#[derive(Debug, Copy, Clone)]
pub struct MidiNote {
    pub on_off: bool,
    /// Channel, should be 0-15
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
}

impl MidiNote {
    pub fn get_raw_note_on_bytes(&self) -> [u8; 3] {
        [
            (8 + self.on_off as u8) * 16 + self.channel,
            self.pitch,
            self.velocity,
        ]
    }
}

pub fn note_to_midi_pitch(note: &Note) -> u8 {
    12 + note.octave * 12 + note.pitch_class.into_u8()
}

pub fn midi_pitch_to_note(pitch: u8) -> Note {
    Note {
        pitch_class: PitchClass::from_u8(pitch),
        octave: (pitch / 12) - 1,
    }
}

pub fn gen_rand_midi_vec(seq_params: &SeqParams, rand_seq: &BaseSeq) -> Vec<Event> {
    let mut rng = rand::thread_rng();
    let mut events_buffer = vec![];

    if let BaseSeq {
        ty: Random(RandomBase { nb_events }),
        id,
        root_note,
        note_len,
    } = rand_seq
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

        let mut step_offset = 0;
        for _ in 0..*nb_events {
            let velocity = rng.gen_range(0..127);
            let pitch = rng.gen_range(0..scale_notes.len());

            let event_midi_on = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: 1,
                    pitch: note_to_midi_pitch(&scale_notes[pitch]),
                    velocity,
                    on_off: true,
                }),
                bar_pos: step_offset,
                id: *id,
            };
            let event_midi_off = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: 1,
                    pitch: note_to_midi_pitch(&scale_notes[pitch]),
                    velocity,
                    on_off: false,
                }),
                bar_pos: (step_offset + *note_len as u32) % seq_params.loop_length,
                id: *id,
            };

            events_buffer.push(event_midi_on);
            events_buffer.push(event_midi_off);
            let time_incr = rng.gen_range(0..seq_params.loop_length);
            step_offset = (step_offset + time_incr) % seq_params.loop_length;
        }
        events_buffer.sort_by_key(|e| e.bar_pos);
    } else {
        eprintln!("Could not insert BaseSeq as its not Random.")
    }

    events_buffer
}

fn gen_euclid(pulses: u32, steps: u32) -> Vec<u8> {
    let head = vec![vec![1u8]; pulses as usize];
    let tail = vec![vec![0u8]; (steps - pulses) as usize];

    fn gen_euclid_rec(mut head: Vec<Vec<u8>>, mut tail: Vec<Vec<u8>>) -> Vec<u8> {
        let mut new_head = vec![];
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
        if tail.len() < 2 {
            return [new_head.concat(), tail.concat()].concat();
        }
        gen_euclid_rec(new_head, tail)
    }

    gen_euclid_rec(head, tail)
}

pub fn gen_euclid_midi_vec(seq_params: &SeqParams, euclid_seq: &BaseSeq) -> Vec<Event> {
    let mut events_buffer = vec![];

    if let BaseSeq {
        ty: Euclid(EuclidBase { pulses, steps }),
        id,
        root_note,
        note_len,
    } = euclid_seq
    {
        if seq_params.loop_length % *steps != 0 {
            eprintln!("Could not generate euclidean rhythm for indivisible loop-length.");
            return events_buffer;
        }

        let euclid_step_len_bar = seq_params.loop_length / *steps;
        let euclid_rhythm = gen_euclid(*pulses, *steps);

        let velocity = 127;
        let pitch = note_to_midi_pitch(root_note);

        let mut time_offset = 0;
        for i in euclid_rhythm {
            if i == 0 {
                continue;
            }

            let event_midi_on = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: 1,
                    pitch,
                    velocity,
                    on_off: true,
                }),
                bar_pos: time_offset,
                id: *id,
            };
            let event_midi_off = Event {
                e_type: EventType::MidiNote(MidiNote {
                    channel: 1,
                    pitch,
                    velocity,
                    on_off: false,
                }),
                bar_pos: (time_offset + *note_len) % seq_params.loop_length,
                id: *id,
            };
            events_buffer.push(event_midi_on);
            events_buffer.push(event_midi_off);
            time_offset += euclid_step_len_bar;
        }
    } else {
        eprintln!("Could not insert BaseSeq as its not Euclidean.")
    }
    events_buffer
}

#[test]
fn test_euclid() {
    assert_eq!(gen_euclid(1, 2), vec![1, 0]);
    assert_eq!(gen_euclid(1, 3), vec![1, 0, 0]);
    assert_eq!(gen_euclid(1, 4), vec![1, 0, 0, 0,]);
    assert_eq!(gen_euclid(4, 12), vec![1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0,]);
    assert_eq!(gen_euclid(2, 3), vec![1, 0, 1]);
    assert_eq!(gen_euclid(2, 5), vec![1, 0, 1, 0, 0]);
    assert_eq!(gen_euclid(3, 4), vec![1, 0, 1, 1]);
}

#[test]
fn test_midi_pitch() {
    assert_eq!(
        note_to_midi_pitch(&Note {
            pitch_class: PitchClass::A,
            octave: 4
        }),
        69
    );
}
