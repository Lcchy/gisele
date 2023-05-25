use anyhow::{anyhow, bail};
use jack::{MidiWriter, ProcessScope};
use num_derive::FromPrimitive;
use parking_lot::{MappedRwLockReadGuard, RwLock, RwLockReadGuard};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand_distr::{Distribution, Normal};
use rust_music_theory::note::Note;
use std::cmp::min;
use std::sync::Arc;
use strum::EnumString;

use crate::jackp::send_event;
use crate::midi::{gen_euclid_midi_vec, gen_rand_midi_vec, note_to_midi_pitch, MidiNote};
use crate::seq::BaseSeqType::{Euclid, Random};

#[derive(Debug, Clone)]
pub struct Event {
    pub e_type: EventType,
    /// Nb bars from sequence start (i.e. position on grid)
    pub bar_pos: f32,
}

impl Event {
    fn _is_note_on_off(&self) -> bool {
        match self.e_type {
            EventType::MidiNoteOn(n) | EventType::MidiNoteOff(n) => n.on_off,
            EventType::_Fill => unimplemented!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EventType {
    MidiNoteOn(MidiNote),
    MidiNoteOff(MidiNote),
    _Fill,
}

pub struct Sequencer {
    /// Write: OSC process, Read: Jack process
    pub params: Arc<RwLock<SeqParams>>,
    /// Current state of the [BaseSeq]s, base sequences from which events are generated
    pub base_seqs: Arc<RwLock<Vec<BaseSeq>>>,
    /// Effect processor state
    pub fx_procs: Arc<RwLock<Vec<FxProcessor>>>,
    /// Internal sequencer parameters
    /// Write: Jack process, Read: OSC process
    pub internal: Arc<RwLock<SeqInternal>>,
}

impl Sequencer {
    pub fn new(bpm: f32) -> Self {
        let seq_params = SeqParams {
            status: SeqStatus::Stop,
            bpm,
            incr: 0,
        };
        Sequencer {
            params: Arc::new(RwLock::new(seq_params)),
            base_seqs: Arc::new(RwLock::new(vec![])),
            internal: Arc::new(RwLock::new(SeqInternal::new())),
            fx_procs: Arc::new(RwLock::new(vec![])),
        }
    }

    pub fn add_base_seq(&self, base_seq_params: BaseSeqParams) -> anyhow::Result<()> {
        let mut seq_params = self.params.write();
        let base_seq = BaseSeq::new_fill(base_seq_params, seq_params.incr, &self.internal.read())?;
        self.base_seqs.write().push(base_seq);
        println!("Inserted base sequence id {}", seq_params.incr);
        seq_params.incr += 1;
        Ok(())
    }

    pub fn add_fx_processor(&self, base_seq_id: u32) -> anyhow::Result<()> {
        let mut seq_params = self.params.write();
        let fx_proc = FxProcessor::new(seq_params.incr);
        let base_seq = self.get_base_seq(base_seq_id)?;
        base_seq.fx_proc_ids.write().push(fx_proc.id);
        self.fx_procs.write().push(fx_proc);
        println!("Inserted fx processor id {}", seq_params.incr);
        seq_params.incr += 1;
        Ok(())
    }

    /// BaseSeq getter, mapping the lock contents in order to preserve the lifetime
    pub fn get_base_seq(&self, base_seq_id: u32) -> anyhow::Result<MappedRwLockReadGuard<BaseSeq>> {
        RwLockReadGuard::try_map(self.base_seqs.read(), |p| {
            p.iter().find(|s| s.id == base_seq_id)
        })
        .map_err(|_| anyhow::format_err!("Base sequence {base_seq_id} could not be found."))
    }

    /// FxProcessor getter, mapping the lock contents in order to preserve the lifetime
    pub fn get_fx_proc(
        &self,
        fx_proc_id: u32,
    ) -> anyhow::Result<MappedRwLockReadGuard<FxProcessor>> {
        RwLockReadGuard::try_map(self.fx_procs.read(), |p| {
            p.iter().find(|f| f.id == fx_proc_id)
        })
        .map_err(|_| anyhow::format_err!("Base sequence {fx_proc_id} could not be found."))
    }

    pub fn regen_base_seq(&self, base_seq_id: u32) -> anyhow::Result<()> {
        let base_seq = self.get_base_seq(base_seq_id)?;
        base_seq.gen_fill(&self.internal.read())?;
        Ok(())
    }

    pub fn change_note_len(&self, base_seq_id: u32, target_note_len: f32) -> anyhow::Result<()> {
        let base_seq = self.get_base_seq(base_seq_id)?;
        base_seq.change_note_len(target_note_len, &self.internal.read())
    }

    pub fn change_loop_len(&self, base_seq_id: u32, target_loop_len: f32) -> anyhow::Result<()> {
        let base_seq = self.get_base_seq(base_seq_id)?;
        base_seq.params.write().loop_length = target_loop_len;
        Ok(())
    }

    pub fn set_nb_events(&self, base_seq_id: u32, target_nb_events: u32) -> anyhow::Result<()> {
        let base_seq = self.get_base_seq(base_seq_id)?;
        base_seq.set_nb_events(target_nb_events, &self.internal.read())?;
        Ok(())
    }

    pub fn transpose(&self, base_seq_id: u32, target_root_note: Note) -> anyhow::Result<()> {
        let base_seq = self.get_base_seq(base_seq_id)?;
        base_seq.transpose(target_root_note)?;
        Ok(())
    }

    /// Delete all BaseSeqs, empty the EventBuffers
    pub fn empty(&self) {
        *self.base_seqs.write() = vec![];
        let mut seq_params = self.params.write();
        seq_params.incr = 0;
    }

    pub fn reset_base_seqs(&self) {
        for base_seq in &*self.base_seqs.read() {
            *base_seq.event_head.write() = 0;
        }
    }

    pub fn notes_off(&self, ps: &ProcessScope, out_buff: &mut MidiWriter) {
        let mut midi_chs = self
            .base_seqs
            .read()
            .iter()
            .map(|b| b.params.read().midi_ch)
            .collect::<Vec<u8>>();
        midi_chs.sort();
        midi_chs.dedup();
        for ch in midi_chs {
            for pitch in 0..128 {
                send_event(
                    ps,
                    out_buff,
                    &Event {
                        e_type: EventType::MidiNoteOff(MidiNote {
                            on_off: false,
                            channel: ch,
                            pitch,
                            velocity: 1u8,
                        }),
                        bar_pos: 0.,
                    },
                )
            }
        }
    }

    pub fn remove_base_seq(&self, base_seq_id: u32) -> anyhow::Result<()> {
        let index = self
            .base_seqs
            .read()
            .iter()
            .position(|b| b.id == base_seq_id)
            .ok_or_else(|| anyhow!("Could not find base sequence of id {base_seq_id}"))?;
        self.base_seqs.write().remove(index);
        Ok(())
    }

    pub fn process_event(&self, proc_ids: &Vec<u32>, event: &mut Event) {
        for fx_proc_id in proc_ids {
            if let Ok(fx_proc) = self.get_fx_proc(*fx_proc_id) {
                fx_proc.process(event);
            }
        }
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
    pub bpm: f32,
    /// Counter of total nb of BaseSeqs/FxProcessor ever created, used for id
    pub incr: u32,
}

//////////////////////////////////////////////////////////////////////////
/// Base Sequences

#[derive(Clone, Debug)]
pub enum BaseSeqType {
    Random(RandomBase),
    Euclid(EuclidBase),
}

#[derive(Clone, Debug)]
pub struct BaseSeqParams {
    pub ty: BaseSeqType,
    /// In bars, 16 is 4 measures
    pub loop_length: f32,
    pub root_note: Note,
    /// In bars
    pub note_len_avg: f32,
    /// Standard deviation from average value note_len in normal random generation
    /// In bars
    pub note_len_div: f32,
    /// In midi range (0-127)
    pub velocity_avg: u8,
    /// Standard deviation from average value velocity in normal random generation
    pub velocity_div: f32,
    /// Channel, should be 1-16
    pub midi_ch: u8,
}

/// State of a base sequence that is generated and inserted into the EventBuffer
pub struct BaseSeq {
    pub params: Arc<RwLock<BaseSeqParams>>,
    /// Current position in the event buffer.
    /// Write: OSC + Jack process
    pub event_head: Arc<RwLock<usize>>,
    /// Identifies events in the EventBuffer
    /// Event Buffer
    /// Events are ordered by their times
    /// Write: OSC process, Read: Jack process
    pub event_buffer: Arc<RwLock<Vec<Event>>>,
    /// FxProcessor ids to which the BaseSeq feeds events
    pub fx_proc_ids: Arc<RwLock<Vec<u32>>>,
    /// Unique identifier to the base_seq
    pub id: u32,
}

impl BaseSeq {
    /// Create a new base sequence and fill its event buffer.
    /// The jack process window end time gives a reference point to the present time for the synchronizing
    /// of the BaseSeq event_head
    fn new_fill(params: BaseSeqParams, id: u32, seq_int: &SeqInternal) -> anyhow::Result<BaseSeq> {
        let base_seq = BaseSeq {
            params: Arc::new(RwLock::new(params)),
            event_head: Arc::new(RwLock::new(0)),
            event_buffer: Arc::new(RwLock::new(vec![])),
            fx_proc_ids: Arc::new(RwLock::new(vec![])),
            id,
        };
        base_seq.gen_fill(seq_int)?;
        Ok(base_seq)
    }

    /// Fill the event buffer of a BaseSeq.
    /// The jack process window end time gives a reference point to the present time for the synchronizing
    /// of the BaseSeq event_head
    fn gen_fill(&self, seq_int: &SeqInternal) -> anyhow::Result<()> {
        //Insert events
        let mut events = match self.params.read().ty {
            Random(_) => gen_rand_midi_vec(self),
            Euclid(_) => gen_euclid_midi_vec(self)?,
        };
        events.sort_by_key(|e| (e.bar_pos * 1_000.) as u32); //TODO use FP32 instead
        *self.event_buffer.write() = events;
        self.sync_event_head(seq_int);
        Ok(())
    }

    fn sync_event_head(&self, seq_int: &SeqInternal) {
        // Reset event_head to next idx right after the current jack window
        // The preliminary binary search is an optional optimization.
        let event_buffer = self.event_buffer.read();
        let mut new_head = match event_buffer.binary_search_by_key(
            &(1_000
                * ((seq_int.j_window_time_end % (self.params.read().loop_length as f64)) as u32)),
            |e| ((e.bar_pos * 1_000.) as u32),
        ) {
            Ok(idx) | Err(idx) => idx,
        };

        if new_head == event_buffer.len() {
            new_head = 0;
        } else if let Some(idx) = event_buffer[new_head..]
            .iter()
            .position(|e| e.bar_pos > event_buffer[new_head].bar_pos)
        {
            // As the return of the binary search for multiple matches is arbitrary,
            // we look for the exact event.
            new_head += idx;
        } else {
            new_head = 0;
        }

        *self.event_head.write() = min(new_head, event_buffer.len().saturating_sub(1));

        println!("Event head synced!")
    }

    pub(self) fn change_note_len(
        &self,
        target_note_len: f32,
        seq_int: &SeqInternal,
    ) -> anyhow::Result<()> {
        let mut params = self.params.write();
        let mut event_buff = self.event_buffer.write();
        for event in event_buff.iter_mut() {
            if let EventType::MidiNoteOn(MidiNote { on_off, .. })
            | EventType::MidiNoteOff(MidiNote { on_off, .. }) = event.e_type
            {
                if !on_off {
                    event.bar_pos = event.bar_pos + target_note_len - params.note_len_avg;
                    event.bar_pos %= params.loop_length;
                }
            }
        }
        params.note_len_avg = target_note_len;

        event_buff.sort_by_key(|e| (e.bar_pos * 1_000.) as u32);
        self.sync_event_head(seq_int);
        Ok(())
    }

    pub(self) fn set_nb_events(
        &self,
        target_nb_events: u32,
        seq_int: &SeqInternal,
    ) -> anyhow::Result<()> {
        let mut params = self.params.write();
        if let BaseSeqParams {
            ty: Random(RandomBase { ref mut nb_events }),
            ..
        } = *params
        {
            *nb_events = target_nb_events;
        } else {
            bail!("The given base_seq_id is wrong.");
        };
        drop(params);
        self.gen_fill(seq_int)?;
        Ok(())
    }

    pub(self) fn transpose(&self, target_root_note: Note) -> anyhow::Result<()> {
        let mut params = self.params.write();
        let root_note_midi = note_to_midi_pitch(&params.root_note);
        let target_root_note_midi = note_to_midi_pitch(&target_root_note);
        let pitch_diff = target_root_note_midi as i32 - root_note_midi as i32;
        for event in self.event_buffer.write().iter_mut() {
            if let EventType::MidiNoteOn(MidiNote { ref mut pitch, .. })
            | EventType::MidiNoteOff(MidiNote { ref mut pitch, .. }) = event.e_type
            {
                *pitch = (*pitch as i32 + pitch_diff).clamp(0, 127) as u8;
            }
        }
        params.root_note = target_root_note;
        Ok(())
    }

    pub fn incr_event_head(&self) {
        let curr_event_head = *self.event_head.read();
        *self.event_head.write() = (curr_event_head + 1) % self.event_buffer.read().len();
    }

    //TODO to be used in when inserting evnets to increase nb_events without regen
    // /// The input events need to be sorted by bar_pos
    // pub fn insert_events(&self, events: Vec<Event>) {
    //     let mut event_buffer_mut = self.event_buffer.write();
    //     let mut buff_idx = 0;
    //     for e in events {
    //         while buff_idx < event_buffer_mut.len() {
    //             if event_buffer_mut[buff_idx].bar_pos < e.bar_pos {
    //                 buff_idx += 1;
    //             } else {
    //                 break;
    //             }
    //         }
    //         event_buffer_mut.insert(buff_idx, e.clone());
    //         buff_idx += 1;
    //     }
    // }
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
/// Effect Event processor

pub struct FxProcessor {
    rng: Arc<RwLock<StdRng>>,
    distr: Normal<f64>,
    // processor: Box<dyn Fn(Event) -> Event>,
    /// Unique identifier to the FxProcessors
    pub id: u32,
}

impl FxProcessor {
    fn new(id: u32) -> Self {
        let rng = Arc::new(RwLock::new(rand::rngs::StdRng::from_entropy()));
        let distr = Normal::new(0., 1.).unwrap();
        // let processor = Box::new(|e| -> return e);
        FxProcessor {
            rng,
            distr,
            // processor,
            id,
        }
    }

    pub(crate) fn process(&self, event: &mut Event) {
        match event.e_type {
            EventType::MidiNote(ref mut note) => {
                let rng_guard = &mut *self.rng.write();
                note.pitch = (note.pitch as f64 + self.distr.sample(rng_guard)) as u8;
            }
            EventType::_Fill => todo!(),
        };
    }
}

//////////////////////////////////////////////////////////////////////////
/// Internal Sequencer state

/// Additional SeqParams, only to be set and read by the jack Cycle
pub struct SeqInternal {
    /// Allows for cycle skipping when on pause/stop.
    /// Indicates, when stopping, if we are on the final cycle before silence.
    /// Only noteOff events are allowed on final cycle.
    pub status: SeqInternalStatus,
    /// Position of current jack cycle in time
    /// In bars. To be reset on loop or start/stop
    pub j_window_time_start: f64,
    /// Position of current jack cycle in time
    /// In bars. To be reset on loop or start/stop
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

    pub fn event_in_cycle(&self, event_time: f64, loop_len: f32) -> bool {
        let win_start_looped = self.j_window_time_start % (loop_len as f64);
        let win_end_looped = self.j_window_time_end % (loop_len as f64);
        if win_start_looped < win_end_looped {
            win_start_looped <= event_time && event_time < win_end_looped
        } else {
            // EventBuffer wrapping case
            println!("Wrapping EventBuffer..");
            win_start_looped <= event_time || event_time < win_end_looped
        }
    }
}
