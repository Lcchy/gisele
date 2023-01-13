use std::{
    sync::{Arc, RwLock},
    vec,
};

use crate::seq::BaseSeqParams::{Euclid, Random};
use num_derive::FromPrimitive;
use rust_music_theory::note::Note;
use strum::EnumString;

use crate::midi::{gen_euclid_midi_vec, gen_rand_midi_vec, note_to_midi_pitch, MidiNote};

pub struct Event {
    pub e_type: EventType,
    /// usec event position from start position
    pub time: u64,
    /// Ties the event to its [BaseSeq]
    pub id: u32,
}

pub enum EventType {
    MidiNote(MidiNote),
    _Fill,
}

pub struct Sequencer {
    /// Write: osc process, Read: Jack process
    pub params: Arc<RwLock<SeqParams>>,
    /// Event Bufffer
    /// Events should be ordered by their times
    /// Write: TBD, Read: Jack process
    pub event_buffer: Arc<RwLock<Vec<Event>>>,
}

impl Sequencer {
    pub fn new(bpm: u16, loop_length: u64) -> Self {
        let seq_params = SeqParams {
            status: SeqStatus::Stop,
            bpm,
            loop_length,
            // note_length: 5,
            base_seqs: vec![],
            base_seq_incr: 0,
        };
        Sequencer {
            event_buffer: Arc::new(RwLock::new(vec![])),
            params: Arc::new(RwLock::new(seq_params)),
        }
    }

    ///The events need to be sorted by their timestamp
    pub fn insert_events(&self, events: Vec<Event>) {
        let mut buff_idx = 0;
        let mut event_buffer_mut = self.event_buffer.write().unwrap();
        if event_buffer_mut.len() == 0 {
            *event_buffer_mut = events;
            return;
        }
        for event in events {
            if event.time < event_buffer_mut[buff_idx].time {
                event_buffer_mut.insert(buff_idx, event);
            } else {
                buff_idx += 1;
                if buff_idx == event_buffer_mut.len() {
                    event_buffer_mut.insert(buff_idx, event);
                }
            }
        }
    }

    pub fn add_base_seq(&self, base_seq_params: BaseSeqParams, root_note: Note, note_len: u8) {
        let mut seq_params = self.params.write().unwrap();
        seq_params.base_seq_incr += 1;
        let base_seq = BaseSeq {
            ty: base_seq_params,
            id: seq_params.base_seq_incr,
            root_note,
            note_len,
        };
        //Insert events
        let events = match base_seq_params {
            Random(_) => gen_rand_midi_vec(&seq_params, &base_seq),
            Euclid(_) => gen_euclid_midi_vec(&seq_params, &base_seq),
        };
        self.insert_events(events);
        seq_params.base_seqs.push(base_seq);
    }

    pub fn rm_base_seq(&self, base_seq: &BaseSeq) {
        let mut event_buffer_mut = self.event_buffer.write().unwrap();
        event_buffer_mut.retain(|e| e.id != base_seq.id);
    }

    // pub fn get_base_seq(&self, base_seq_id: u32) -> Option<&BaseSeq> {
    //     let seq_params = self.params.read().unwrap();
    //     seq_params.base_seqs.iter().find(|s| s.id == base_seq_id)
    // }

    // pub fn get_base_seq_mut(&self, base_seq_id: u32) -> Option<&mut BaseSeq> {
    //     let mut seq_params = self.params.write().unwrap();
    //     seq_params
    //         .base_seqs
    //         .iter_mut()
    //         .find(|s| s.id == base_seq_id)
    // }

    pub fn regen_base_seq(&self, base_seq: &BaseSeq) {
        let seq_params = self.params.read().unwrap();
        self.rm_base_seq(base_seq);
        let regen = match base_seq.ty {
            BaseSeqParams::Random(_) => gen_rand_midi_vec(&seq_params, base_seq),
            BaseSeqParams::Euclid(_) => gen_euclid_midi_vec(&seq_params, base_seq),
        };
        self.insert_events(regen);
    }

    pub fn transpose(&self, base_seq: &mut BaseSeq, target_root_note: Note) {
        let root_note_midi = note_to_midi_pitch(&base_seq.root_note);
        let target_root_note_midi = note_to_midi_pitch(&target_root_note);
        let pitch_diff = target_root_note_midi as i32 - root_note_midi as i32;
        let mut event_buffer_mut = self.event_buffer.write().unwrap();
        for event in event_buffer_mut.iter_mut() {
            if event.id == base_seq.id {
                if let EventType::MidiNote(MidiNote { ref mut pitch, .. }) = event.e_type {
                    *pitch = (*pitch as i32 + pitch_diff).clamp(0, 127) as u8;
                }
            }
        }
        base_seq.root_note = target_root_note;
    }

    /// Delete all BaseSeqs, empty the EventBuffer
    pub fn empty(&self) {
        let mut event_buffer_mut = self.event_buffer.write().unwrap();
        *event_buffer_mut = vec![];
        let mut seq_params = self.params.write().unwrap();
        seq_params.base_seqs = vec![];
        seq_params.base_seq_incr = 0;
    }
}

//TODO proper density input function
// // Should be 0<=..<1
// let event_density = 0.3f64;
// // Capping at 1 event every 10 us
// let nb_events = min(
//     -(1. - event_density).ln(),
//     loop_length_arc.as_ref().read().unwrap().checked_div(10.),
// );

#[derive(Clone, PartialEq, Eq, EnumString, Debug, FromPrimitive)]
pub enum SeqStatus {
    Stop,
    Start,
    Pause,
}

pub struct SeqParams {
    pub status: SeqStatus,
    pub bpm: u16,
    /// In bars, 16 is 4 measures
    pub loop_length: u64,
    /// Current state of the [BaseSeq]s that constitute the EventBuffer
    pub base_seqs: Vec<BaseSeq>,
    /// Counter of total nb of BaseSeqs ever created, used for [BaseSeq] id
    pub base_seq_incr: u32,
}

impl SeqParams {
    pub fn get_loop_len_in_us(&self) -> u64 {
        ((self.loop_length as f64) * 60_000_000. / self.bpm as f64) as u64
    }

    pub fn get_step_len_in_us(&self) -> u64 {
        (60_000_000. / self.bpm as f64) as u64
    }
}

//////////////////////////////////////////////////////////////////////////
/// Base Sequences

#[derive(Clone, Copy)]
pub enum BaseSeqParams {
    Random(RandomBase),
    Euclid(EuclidBase),
}

/// State of a base sequence that is generated and inserted into the EventBuffer
pub struct BaseSeq {
    pub ty: BaseSeqParams,
    /// Identifies events in the EventBuffer
    pub id: u32,
    pub root_note: Note,
    /// In percent, 100 is loop_length / 2
    pub note_len: u8,
}

#[derive(Clone, Copy)]
pub struct RandomBase {
    pub nb_events: u64,
}

#[derive(Clone, Copy)]
pub struct EuclidBase {
    pub pulses: u8, //Could be more?
    pub steps: u8,
}

//////////////////////////////////////////////////////////////////////////
/// Internal Sequencer state

/// Additionnal SeqParams, only to be set and read by the jack Cycle
pub struct SeqInternal {
    /// Indicates, when stopping, if we are on the final cycle before silence.
    /// Only allowing noteOff events on final cycle.
    /// Allows for cycle skipping when on pause/stop.
    pub status: SeqInternalStatus,
    /// Current position in the event buffer.
    pub event_head: usize,
    /// Position of current jack cycle in sequencing time loop.
    /// In usecs. To be reset on loop or start/stop
    pub j_window_time_start: u64,
    /// Position of current jack cycle in sequencing time loop.
    /// In usecs. To be reset on loop or start/stop
    pub j_window_time_end: u64,
}

#[derive(PartialEq, Eq)]
pub enum SeqInternalStatus {
    Silence,
    Playing,
}

impl SeqInternal {
    pub fn new() -> Self {
        SeqInternal {
            status: SeqInternalStatus::Silence,
            event_head: 0,
            j_window_time_start: 0,
            j_window_time_end: 0,
        }
    }
    pub fn stop_reset(&mut self) {
        self.event_head = 0;
        self.j_window_time_start = 0;
        self.j_window_time_end = 0;
    }
    pub fn event_in_cycle(&self, event_time: u64) -> bool {
        // println!(
        //     "Window start {} | Window end {}",
        //     self.j_window_time_start, self.j_window_time_end
        // );
        if self.j_window_time_start < self.j_window_time_end {
            self.j_window_time_start <= event_time && event_time < self.j_window_time_end
        } else {
            // EventBuffer wrapping case
            println!("Wrapping EventBuffer..");
            self.j_window_time_start <= event_time || event_time < self.j_window_time_end
        }
    }
}
