use anyhow::bail;
use num_derive::FromPrimitive;
use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use rust_music_theory::note::Note;
use std::cmp::min;
use std::sync::Arc;
use strum::EnumString;

use crate::midi::{gen_euclid_midi_vec, gen_rand_midi_vec, note_to_midi_pitch, MidiNote};
use crate::seq::BaseSeqParams::{Euclid, Random};

#[derive(Debug, Clone)]
pub struct Event {
    pub e_type: EventType,
    /// Nb bars from sequence start (i.e. position on bpm grid)
    pub bar_pos: u32,
    /// Ties the event to its [BaseSeq]
    pub id: u32,
}

impl Event {
    fn is_note_on_off(&self) -> bool {
        match self.e_type {
            EventType::MidiNote(n) => n.on_off,
            EventType::_Fill => unimplemented!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EventType {
    MidiNote(MidiNote),
    _Fill,
}

pub struct Sequencer {
    /// Write: OSC process, Read: Jack process
    pub params: Arc<RwLock<SeqParams>>,
    /// Current state of the [BaseSeq]s that constitute the EventBuffer
    pub base_seqs: Arc<RwLock<Vec<BaseSeq>>>,
    /// Current position in the event buffer.
    /// Write: OSC + Jack processes
    pub event_head: Arc<RwLock<usize>>,
    /// Internal sequencer parameters, only accessed by the Jack loop
    pub internal: Arc<RwLock<SeqInternal>>,
    /// Event Buffer
    /// Events are ordered by their times
    /// Write: OSC process, Read: Jack process
    pub event_buffer: Arc<RwLock<Vec<Event>>>,
}

impl Sequencer {
    pub fn new(bpm: u16, loop_length: u32) -> Self {
        let seq_params = SeqParams {
            status: SeqStatus::Stop,
            bpm,
            loop_length,
            // note_length: 5,
            base_seq_incr: 0,
        };
        Sequencer {
            params: Arc::new(RwLock::new(seq_params)),
            base_seqs: Arc::new(RwLock::new(vec![])),
            event_head: Arc::new(RwLock::new(0)),
            internal: Arc::new(RwLock::new(SeqInternal::new())),
            event_buffer: Arc::new(RwLock::new(vec![])),
        }
    }

    ///The events need to be sorted by their time position
    pub fn insert_events(&self, events: Vec<Event>) {
        println!(
            "Notes to insert {:#?}",
            events
                .iter()
                .map(|e| (e.bar_pos, e.is_note_on_off()))
                .collect::<Vec<(u32, bool)>>()
        );
        let mut event_buffer_mut = self.event_buffer.write();
        println!(
            "Event buffer BEFORE {:#?}",
            event_buffer_mut
                .iter()
                .map(|e| (e.bar_pos, e.is_note_on_off()))
                .collect::<Vec<(u32, bool)>>()
        );

        let mut buff_idx = 0;
        for e in events {
            while buff_idx < event_buffer_mut.len() {
                if event_buffer_mut[buff_idx].bar_pos < e.bar_pos {
                    buff_idx += 1;
                }
            }
            event_buffer_mut.insert(buff_idx, e.clone());
            buff_idx += 1;
        }

        println!(
            "Event buffer AFTER {:#?}",
            event_buffer_mut
                .iter()
                .map(|e| (e.bar_pos, e.is_note_on_off()))
                .collect::<Vec<(u32, bool)>>()
        );
    }

    pub fn add_base_seq(&self, base_seq_params: BaseSeqParams, root_note: Note, note_len: u32) {
        let mut seq_params = self.params.write();
        let base_seq = BaseSeq {
            ty: base_seq_params,
            id: seq_params.base_seq_incr,
            root_note,
            note_len,
        };
        println!("Inserted base sequence id {}", seq_params.base_seq_incr);
        seq_params.base_seq_incr += 1;
        //Insert events
        let events = match base_seq_params {
            Random(_) => gen_rand_midi_vec(&seq_params, &base_seq),
            Euclid(_) => gen_euclid_midi_vec(&seq_params, &base_seq),
        };
        self.insert_events(events);
        self.base_seqs.write().push(base_seq);
    }

    /// BaseSeq getter, mapping the lock contents in order to preserve the lifetime
    pub fn get_base_seq(&self, base_seq_id: u32) -> anyhow::Result<MappedRwLockReadGuard<BaseSeq>> {
        RwLockReadGuard::try_map(self.base_seqs.read(), |p| {
            p.iter().find(|s| s.id == base_seq_id)
        })
        .map_err(|_| anyhow::format_err!("Base sequence could not be found."))
    }

    /// BaseSeq mutable getter, mapping the lock contents in order to preserve the lifetime
    pub fn get_base_seq_mut(
        &self,
        base_seq_id: u32,
    ) -> anyhow::Result<MappedRwLockWriteGuard<BaseSeq>> {
        RwLockWriteGuard::try_map(self.base_seqs.write(), |p| {
            p.iter_mut().find(|s| s.id == base_seq_id)
        })
        .map_err(|_| anyhow::format_err!("Base sequence could not be found."))
    }

    pub fn rm_base_seq_events(&self, base_seq_id: u32) {
        self.event_buffer.write().retain(|e| e.id != base_seq_id);
    }

    pub fn regen_base_seq(&self, base_seq_id: u32) -> anyhow::Result<()> {
        let base_seq = self.get_base_seq(base_seq_id)?;
        self._regen_base_seq(&base_seq);
        Ok(())
    }

    fn _regen_base_seq(&self, base_seq: &BaseSeq) {
        self.rm_base_seq_events(base_seq.id);
        let seq_params = self.params.read();
        let regen = match base_seq.ty {
            BaseSeqParams::Random(_) => gen_rand_midi_vec(&seq_params, base_seq),
            BaseSeqParams::Euclid(_) => gen_euclid_midi_vec(&seq_params, base_seq),
        };
        self.insert_events(regen);
        self.sync_event_head();
    }

    fn sync_event_head(&self) {
        // Reset event_head to next idx right after the current jack window
        let event_buff = self.event_buffer.read();
        match event_buff
            .binary_search_by_key(&(self.internal.read().j_window_time_end as u32 + 1), |e| {
                e.bar_pos
            }) {
            Ok(idx) | Err(idx) => *self.event_head.write() = min(idx, event_buff.len() - 1),
        }
        println!("Event head synced!")
    }

    pub fn set_nb_events(&self, base_seq_id: u32, target_nb_events: u32) -> anyhow::Result<()> {
        println!("Regenerating base sequence..");
        let mut base_seq_mut = self.get_base_seq_mut(base_seq_id)?;
        if let BaseSeq {
            ty: Random(RandomBase { ref mut nb_events }),
            ..
        } = *base_seq_mut
        {
            *nb_events = target_nb_events;
        } else {
            bail!("The given base_seq_id is wrong.");
        };
        self._regen_base_seq(&base_seq_mut);
        Ok(())
    }

    pub fn change_note_len(&self, base_seq_id: u32, target_note_len: u32) -> anyhow::Result<()> {
        let loop_len = self.params.read().loop_length;
        let mut base_seq_mut = self.get_base_seq_mut(base_seq_id)?;

        for event in self.event_buffer.write().iter_mut() {
            if let EventType::MidiNote(MidiNote { on_off, .. }) = event.e_type {
                if event.id == base_seq_id && !on_off {
                    event.bar_pos = (event.bar_pos as i32 + target_note_len as i32
                        - base_seq_mut.note_len as i32) as u32;
                    event.bar_pos %= loop_len;
                }
            }
        }
        base_seq_mut.note_len = target_note_len;

        self.event_buffer.write().sort_by_key(|e| e.bar_pos);
        self.sync_event_head();
        Ok(())
    }

    pub fn transpose(&self, base_seq_id: u32, target_root_note: Note) -> anyhow::Result<()> {
        let mut base_seq_mut = self.get_base_seq_mut(base_seq_id)?;
        let root_note_midi = note_to_midi_pitch(&base_seq_mut.root_note);
        let target_root_note_midi = note_to_midi_pitch(&target_root_note);
        let pitch_diff = target_root_note_midi as i32 - root_note_midi as i32;
        for event in self.event_buffer.write().iter_mut() {
            if event.id == base_seq_mut.id {
                if let EventType::MidiNote(MidiNote { ref mut pitch, .. }) = event.e_type {
                    *pitch = (*pitch as i32 + pitch_diff).clamp(0, 127) as u8;
                }
            }
        }
        base_seq_mut.root_note = target_root_note;
        Ok(())
    }

    /// Delete all BaseSeqs, empty the EventBuffer
    pub fn empty(&self) {
        *self.event_buffer.write() = vec![];
        *self.base_seqs.write() = vec![];
        let mut seq_params = self.params.write();
        seq_params.base_seq_incr = 0;
    }

    pub fn stop_reset(&self, mut seq_int_lock: RwLockWriteGuard<SeqInternal>) {
        *self.event_head.write() = 0;
        seq_int_lock.j_window_time_start = 0.;
        seq_int_lock.j_window_time_end = 0.;
    }

    pub fn incr_event_head(&self) {
        let curr_event_head = *self.event_head.read();
        *self.event_head.write() = (curr_event_head + 1) % self.event_buffer.read().len();
    }
}

#[derive(Clone, PartialEq, Eq, EnumString, Debug, FromPrimitive)]
pub enum SeqStatus {
    /// Pause ans reset sequencer to start position
    Stop,
    Start,
    Pause,
    /// Sequencer is set to shutdown gracefully
    Shutdown,
}

pub struct SeqParams {
    pub status: SeqStatus,
    pub bpm: u16,
    /// In bars, 16 is 4 measures
    pub loop_length: u32,
    /// Counter of total nb of BaseSeqs ever created, used for [BaseSeq] id
    pub base_seq_incr: u32,
}

//////////////////////////////////////////////////////////////////////////
/// Base Sequences

#[derive(Clone, Copy, Debug)]
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
    /// In bars
    pub note_len: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct RandomBase {
    pub nb_events: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct EuclidBase {
    pub pulses: u32,
    pub steps: u32,
}

//////////////////////////////////////////////////////////////////////////
/// Internal Sequencer state

/// Additional SeqParams, only to be set and read by the jack Cycle
pub struct SeqInternal {
    /// Allows for cycle skipping when on pause/stop.
    /// Indicates, when stopping, if we are on the final cycle before silence.
    /// Only noteOff events are allowed on final cycle.
    pub status: SeqInternalStatus,
    /// Position of current jack cycle in sequencing time loop.
    /// In usecs. To be reset on loop or start/stop
    pub j_window_time_start: f64,
    /// Position of current jack cycle in sequencing time loop.
    /// In usecs. To be reset on loop or start/stop
    pub j_window_time_end: f64,
    /// Current bar position in loop rhythm grid.
    /// Stored here for logging purposes
    pub curr_bar: u32,
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
            j_window_time_start: 0.,
            j_window_time_end: 0.,
            curr_bar: 0,
        }
    }

    pub fn event_in_cycle(&self, event_time: f64) -> bool {
        // println!(
        //     "Window start {} | Event Time {} | Window end {}",
        //     self.j_window_time_start, event_time, self.j_window_time_end
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
