use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use std::{
    io,
    sync::{Arc, RwLock},
};

struct MidiNote {
    on_off: bool,
    /// Channel, should be 0-15
    channel: u8,
    pitch: u8,
    velocity: u8,
    // / usec
    // len: u64,
}

impl MidiNote {
    fn get_raw_note_on_bytes(&self) -> [u8; 3] {
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

struct Event {
    e_type: EventType,
    /// usec event position from start position
    time: u64,
}

enum EventType {
    MidiNote(MidiNote),
}

type EventBuffer = Vec<Event>;

#[derive(Clone)]
struct Params {
    /// Current position in the event buffer.
    /// Write: jack process, Read: -
    event_head: usize,
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    curr_time_start: u64,
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    curr_time_end: u64,
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
            e_type: EventType::MidiNote(MidiNote {
                channel: 0,
                pitch: 60,
                velocity: 64,
                on_off: true,
            }),
            time: 0,
        },
        Event {
            e_type: EventType::MidiNote(MidiNote {
                channel: 0,
                pitch: 60,
                velocity: 64,
                on_off: false,
            }),
            time: 500_000,
        },
    ]));
    let bpm_arc = Arc::new(RwLock::new(120));
    let loop_length_arc = Arc::new(RwLock::new(2_000_000)); // 2sec = 4 bars at 120 bpm
    let params_arc = Params {
        event_head: 0,
        curr_time_start: 0,
        curr_time_end: 0,
        event_buffer: event_buffer_arc,
        bpm: bpm_arc,
        loop_length: loop_length_arc,
    };
    let mut params_ref = params_arc.clone();

    // Define the Jack process
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        // Max event buff size was measured as 32736
        let mut out_buff = out_port.writer(ps);

        let loop_len = params_ref.loop_length.read().unwrap();
        let event_buffer = params_ref.event_buffer.read().unwrap();
        let next_event = &event_buffer[params_ref.event_head];

        let cy_times = ps.cycle_times().unwrap();
        params_ref.curr_time_end =
            (params_ref.curr_time_end + (cy_times.next_usecs - cy_times.current_usecs)) % *loop_len;

        println!("next_event.time {}", next_event.time);
        println!("Curr time {}", params_ref.curr_time_end);

        // This shitty check should be removed once we map events to frames directly
        let push_event = if params_ref.curr_time_start < params_ref.curr_time_end {
            params_ref.curr_time_start <= next_event.time
                && next_event.time < params_ref.curr_time_end
        } else {
            // Wrapping case
            params_ref.curr_time_start <= next_event.time
                || next_event.time < params_ref.curr_time_end
        };

        if push_event {
            match next_event.e_type {
                EventType::MidiNote(ref note) => {
                    let raw_midi = RawMidi {
                        //TODO add some frames here for precise timing, as a process cycle is 42ms, see jack doc
                        // This should allow to map events on specific frames, making the above if condition redundant
                        time: ps.frames_since_cycle_start(),
                        // bytes: &[144, 60, 64],
                        bytes: &note.get_raw_note_on_bytes(),
                    };
                    // println!("{:?}", note.get_raw_note_on_bytes());
                    out_buff.write(&raw_midi).unwrap();
                    println!("Sending note {:?}", &note.get_raw_note_on_bytes());
                }
            }
            params_ref.event_head = (params_ref.event_head + 1) % event_buffer.len();
        }
        params_ref.curr_time_start = params_ref.curr_time_end;

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
