use rand::Rng;
use rust_music_theory::{
    note::{Note, Notes, PitchClass},
    scale::{Direction, Mode, Scale, ScaleType},
};

use crate::{
    seq::{Event, SeqParams},
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

fn note_to_midi_pitch(note: &Note) -> u8 {
    12 + note.octave * 12 + note.pitch_class.into_u8()
}

pub fn midi_pitch_to_note(pitch: u8) -> Note {
    Note {
        pitch_class: PitchClass::from_u8(pitch),
        octave: (pitch / 12) - 1,
    }
}

pub fn gen_rand_midi_vec(seq_params: &SeqParams) -> Vec<Event> {
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
    let rythm_precision = 16; // =16 -> 16th note, 1 note = 4bpm taps
    let rythm_atom_duration = 4 * 60_000_000 / (rythm_precision * seq_params.bpm) as u64; // In usecs
    let nb_rythm_atoms = seq_params.loop_length / rythm_atom_duration;

    for _ in 0..seq_params.nb_events {
        let velocity = rng.gen_range(95..127);
        let pitch = rng.gen_range(0..scale_notes.len());
        let rythm_offset = rythm_atom_duration * rng.gen_range(0..nb_rythm_atoms);
        let note_len = rythm_atom_duration
            * rng.gen_range(0..nb_rythm_atoms * seq_params.note_length as u64 / (2 * 100));
        let event_midi_on = Event {
            e_type: EventType::MidiNote(MidiNote {
                channel: 1,
                pitch: note_to_midi_pitch(&scale_notes[pitch]),
                velocity,
                on_off: true,
            }),
            time: rythm_offset,
        };
        let event_midi_off = Event {
            e_type: EventType::MidiNote(MidiNote {
                channel: 1,
                pitch: note_to_midi_pitch(&scale_notes[pitch]),
                velocity,
                on_off: false,
            }),
            // % could be a problem, wrapping a quantized note_len when loop_len is off quantization, ie it will end off beat
            time: (rythm_offset + note_len) % seq_params.loop_length,
        };
        events_buffer.push(event_midi_on);
        events_buffer.push(event_midi_off);
    }
    events_buffer.sort_by_key(|e| e.time);
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
