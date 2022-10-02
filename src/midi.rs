use rand::Rng;
use rust_music_theory::{
    note::{Note, Notes, PitchClass},
    scale::{Direction, Mode, Scale, ScaleType},
};

use crate::{Event, EventType};

#[derive(Debug)]
pub struct MidiNote {
    pub on_off: bool,
    /// Channel, should be 0-15
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
    // / usec
    // len: u64,
}

// impl Debug for MidiNote {

// }

impl MidiNote {
    pub fn get_raw_note_on_bytes(&self) -> [u8; 3] {
        [
            (8 + self.on_off as u8) * 16 + self.channel,
            self.pitch,
            self.velocity,
        ]
    }

    // fn get_raw_note_on<'a, 'b>(&'a self, cycle_frames: u32) -> RawMidi<'b>
    // where
    //     'a: 'b,
    // {
    //     RawMidi {
    //         time: cycle_frames,
    //         bytes: &[9 * 16 + self.channel, self.pitch, self.velocity],
    //         // bytes: &self.get_raw_note_on_bytes().to_owned(),
    //     }
    // }
}

fn note_to_midi_pitch(note: &Note) -> u8 {
    12 + note.octave * 12 + note.pitch_class.into_u8()
}

pub fn gen_rand_midi_vec(bpm: u16, loop_len: u64, nb_events: u64) -> Vec<Event> {
    let mut rng = rand::thread_rng();
    let mut events_buffer = vec![];

    // Harmonic quantization
    let scale = Scale::new(
        ScaleType::Diatonic,
        PitchClass::C,
        4,
        Some(Mode::Ionian),
        Direction::Ascending,
    )
    .unwrap();
    let scale_notes = scale.notes();

    // Rythmic quantization
    let rythm_precision = 1; // =16 -> 16th note, 1 note = 4bpm taps
    let rythm_atom_duration = 4 * 60_000_000 / (rythm_precision * bpm) as u64; // In usecs
    let nb_rythm_atoms = loop_len / rythm_atom_duration;

    for _ in 0..nb_events {
        let velocity = rng.gen_range(0..127);
        let pitch = rng.gen_range(0..scale_notes.len());
        let rythm_offset = rythm_atom_duration * rng.gen_range(0..nb_rythm_atoms);
        let note_len = rythm_atom_duration * rng.gen_range(0..nb_rythm_atoms / 2); //TODO set
        let event_midi_on = Event {
            e_type: EventType::MidiNote(MidiNote {
                channel: 0,
                pitch: note_to_midi_pitch(&scale_notes[pitch]),
                velocity,
                on_off: true,
            }),
            time: rythm_offset,
        };
        let event_midi_off = Event {
            e_type: EventType::MidiNote(MidiNote {
                channel: 0,
                pitch: note_to_midi_pitch(&scale_notes[pitch]),
                velocity,
                on_off: false,
            }),
            // % could be a problem, wrapping a quantized note_len when loop_len is off quantization, ie it will end off beat
            time: (rythm_offset + note_len) % loop_len,
        };
        events_buffer.push(event_midi_on);
        events_buffer.push(event_midi_off);
    }
    events_buffer
}

#[test]
fn test() {
    assert_eq!(
        note_to_midi_pitch(&Note {
            pitch_class: PitchClass::A,
            octave: 4
        }),
        69
    );
}
