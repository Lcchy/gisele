use std::sync::{Arc, RwLock};

use num_derive::FromPrimitive;
use strum::EnumString;

use crate::midi::{gen_rand_midi_vec, MidiNote};

pub struct Sequencer {
    /// Write: osc process, Read: Jack process
    pub params: Arc<RwLock<SeqParams>>,
    /// Event Bufffer
    /// Events should be ordered by their times
    /// Write: TBD, Read: Jack process
    pub event_buffer: Arc<RwLock<Vec<Event>>>,
}

impl Sequencer {
    pub fn new(bpm: u16, loop_length: u64, nb_events: u64) -> Self {
        let seq_params = SeqParams {
            status: SeqStatus::Stop,
            bpm,
            loop_length,
            nb_events,
        };
        let event_buffer =
            gen_rand_midi_vec(seq_params.bpm, seq_params.loop_length, seq_params.nb_events);
        Sequencer {
            event_buffer: Arc::new(RwLock::new(event_buffer)),
            params: Arc::new(RwLock::new(seq_params)),
        }
    }

    pub fn reseed(&self) {
        let seq_params = self.params.read().unwrap();
        let mut event_buffer_mut = self.event_buffer.write().unwrap();
        *event_buffer_mut =
            gen_rand_midi_vec(seq_params.bpm, seq_params.loop_length, seq_params.nb_events);
    }
}

pub struct Event {
    pub e_type: EventType,
    /// usec event position from start position
    pub time: u64,
}

pub enum EventType {
    MidiNote(MidiNote),
}

#[derive(Clone, PartialEq, EnumString, Debug, FromPrimitive)]
pub enum SeqStatus {
    Stop,
    Start,
    Pause,
}

//TODO proper density input function
// // Should be 0<=..<1
// let event_density = 0.3f64;
// // Capping at 1 event every 10 us
// let nb_events = min(
//     -(1. - event_density).ln(),
//     loop_length_arc.as_ref().read().unwrap().checked_div(10.),
// );

pub struct SeqParams {
    pub status: SeqStatus,
    pub bpm: u16,
    /// In usecs,//TODO to be quantized to whole note on bpm, with option to deviate
    pub loop_length: u64,
    pub nb_events: u64,
    // density
}

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

#[derive(PartialEq)]
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