use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use std::{
    io,
    sync::{Arc, RwLock},
};

struct MidiNote {
    // on_off: bool,
    /// Channel, should be 0-15
    channel: u8,
    pitch: u8,
    velocity: u8,
    // / usec
    // len: u64,
}

impl MidiNote {
    fn get_raw_note_on_bytes(&self) -> [u8; 3] {
        [9 * 16 + self.channel, self.pitch, self.velocity]
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

struct Event {
    e_type: EventType,
    /// usec event position from start position
    time: u64,
}

enum EventType {
    MidiNoteOn(MidiNote),
    MidiNoteOff(MidiNote),
}

type EventBuffer = Vec<Event>;

#[derive(Clone)]
struct Params {
    /// Current position in the event buffer.
    /// Write: jack process, Read: -
    event_head: usize,
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    curr_time: u64,
    /// Write: osc process, Read: Jack process
    bpm: Arc<RwLock<u16>>,
    /// In usecs
    /// Write: osc process, Read: Jack process
    loop_length: Arc<RwLock<u64>>,
    /// Write: osc process, Read: Jack process
    event_buffer: Arc<RwLock<EventBuffer>>,
    //TODO maybe bundle RwLocks?
}

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let mut out_port = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    //TODO Use BPM and some current_time in usec to compute if note should be played in current cycle (and at which frame?), start at 1 per BPM
    // Print period_time, estimate if cycle is short enough for ms precision of onset
    // TODO LATER: have a central sequencer process that pushes out events to jack midi or osc sender

    // Init values
    let event_buffer_arc = Arc::new(RwLock::new(vec![
        // TODO builder function
        Event {
            e_type: EventType::MidiNoteOn(MidiNote {
                channel: 0,
                pitch: 60,
                velocity: 64,
                // len: 1_000_000,
            }),
            time: 0,
        },
        Event {
            e_type: EventType::MidiNoteOff(MidiNote {
                channel: 0,
                pitch: 60,
                velocity: 64,
                // len: 1_000_000,
            }),
            time: 500_000,
        },
    ]));
    let bpm_arc = Arc::new(RwLock::new(120));
    let loop_length_arc = Arc::new(RwLock::new(16));
    let params_arc = Params {
        event_head: 0,
        curr_time: 0,
        event_buffer: event_buffer_arc,
        bpm: bpm_arc,
        loop_length: loop_length_arc,
    };
    let mut params_ref = params_arc.clone();

    // Define the Jack process
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let mut out_buff = out_port.writer(ps);
        // out_buff.max_event_size()

        let cy_times = ps.cycle_times().unwrap();

        println!("Period {}", cy_times.current_frames);

        let loop_len = params_ref.loop_length.read().unwrap();
        let event_buffer = params_ref.event_buffer.read().unwrap();
        let next_event = &event_buffer[params_ref.event_head];

        // if next_event.time < cy_times.next_usecs % *loop_len {
        // match next_event.e_type {
        // EventType::MidiNoteOn(ref note) | EventType::MidiNoteOff(ref note) => {
        let raw_midi = RawMidi {
            //TODO add some frames here for precise timing, as a process cycle is 42ms, see jack doc
            time: ps.frames_since_cycle_start(),
            bytes: &[144, 60, 64],
            // bytes: &note.get_raw_note_on_bytes(),
        };
        // println!("{:?}", note.get_raw_note_on_bytes());
        out_buff.write(&raw_midi).unwrap();
        // }
        // }
        // params_ref.event_head = (params_ref.event_head + 1) % event_buffer.len();
        // }

        jack::Control::Continue
    };

    // Start the Jack thread
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Wait for user input to quit
    println!("Press enter/return to quit...");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
    active_client.deactivate().unwrap();

    Ok(())
}
