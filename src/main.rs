use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use midi::MidiNote;
use num_derive::FromPrimitive;
use std::{
    io,
    net::UdpSocket,
    sync::{Arc, RwLock},
    thread,
};
use strum::EnumString;

use crate::{
    midi::gen_rand_midi_vec,
    osc::{osc_process_closure, OSC_PORT},
};

mod midi;
mod osc;

pub struct Event {
    e_type: EventType,
    /// usec event position from start position
    time: u64,
}

enum EventType {
    MidiNote(MidiNote),
}

type EventBuffer = Vec<Event>;

#[derive(Clone, PartialEq, EnumString, Debug, FromPrimitive)]
enum SeqStatus {
    Stop,
    Start,
    Pause,
}

pub struct SeqParams {
    status: SeqStatus,
    bpm: u16,
    /// In usecs,//TODO to be quantized to whole note on bpm, with option to deviate
    loop_length: u64,
    nb_events: u64,
    // density
}

pub struct Params {
    /// Write: osc process, Read: Jack process
    seq_params: Arc<RwLock<SeqParams>>,
    /// Events should be ordered by their times
    /// Write: TBD, Read: Jack process
    event_buffer: Arc<RwLock<EventBuffer>>,
}

/// Additionnal SeqParams, only to be set and read by the jack Cycle
struct SeqInternal {
    /// Indicates, when stopping, if we are on the final cycle before silence.
    /// Only allowing noteOff events on final cycle.
    /// Allows for cycle skipping when on pause/stop.
    status: SeqInternalStatus,
    /// Current position in the event buffer.
    /// Write: jack process, Read: -
    event_head: usize,
    /// Position of current jack cycle in sequencing time loop.
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    j_window_time_start: u64,
    /// Position of current jack cycle in sequencing time loop.
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    j_window_time_end: u64,
}

#[derive(PartialEq)]
enum SeqInternalStatus {
    Silence,
    Playing,
}

impl SeqInternal {
    fn stop_reset(&mut self) {
        self.event_head = 0;
        self.j_window_time_start = 0;
        self.j_window_time_end = 0;
    }
}

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let mut out_port = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    // TODO LATER: have a central sequencer process that pushes out events to jack midi or osc sender

    // Init values
    //TODO proper density input function
    // // Should be 0<=..<1
    // let event_density = 0.3f64;
    // // Capping at 1 event every 10 us
    // let nb_events = min(
    //     -(1. - event_density).ln(),
    //     loop_length_arc.as_ref().read().unwrap().checked_div(10.),
    // );
    let nb_events = 100;
    let loop_length = 20_000_000; //2sec = 4 bars at 120 bpm
    let bpm = 120;
    let mut event_buffer = gen_rand_midi_vec(bpm, loop_length, nb_events);
    event_buffer.sort_by_key(|e| e.time);
    let params_arc = Arc::new(Params {
        event_buffer: Arc::new(RwLock::new(event_buffer)),
        seq_params: Arc::new(RwLock::new(SeqParams {
            status: SeqStatus::Stop,
            bpm,
            loop_length, //2sec = 4 bars at 120 bpm
            nb_events,
        })),
    });
    let params_ref = params_arc.clone();
    let mut seq_int = SeqInternal {
        status: SeqInternalStatus::Silence,
        event_head: 0,
        j_window_time_start: 0,
        j_window_time_end: 0,
    };

    // Define the Jack process
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let seq_params = params_ref.seq_params.read().unwrap();

        // Handle Sequencer statuses
        if seq_params.status == SeqStatus::Start {
            seq_int.status = SeqInternalStatus::Playing;
        }
        if seq_int.status == SeqInternalStatus::Silence {
            return jack::Control::Continue;
        }

        let event_buffer = &*params_ref.event_buffer.read().unwrap();
        let mut out_buff = out_port.writer(ps);
        let loop_len = seq_params.loop_length;
        let cy_times = ps.cycle_times().unwrap();

        seq_int.j_window_time_end =
            (seq_int.j_window_time_end + (cy_times.next_usecs - cy_times.current_usecs)) % loop_len;

        // println!("next_event.time {}", next_event.time);
        // println!("Curr time {}", params_ref.curr_time_end);
        // println!("Curr frames {}", cy_times.current_frames);
        // println!("frames sunce start {}", ps.frames_since_cycle_start());
        // println!("frames sunce start {}", ps.frames_since_cycle_start());

        let event_head_before = seq_int.event_head;
        let halting = (seq_params.status == SeqStatus::Pause
            || seq_params.status == SeqStatus::Stop)
            && seq_int.status == SeqInternalStatus::Playing;
        loop {
            let next_event = &event_buffer[seq_int.event_head];
            // This shitty check should be removed once we map events to frames directly
            let mut push_event = if seq_int.j_window_time_start < seq_int.j_window_time_end {
                seq_int.j_window_time_start <= next_event.time
                    && next_event.time < seq_int.j_window_time_end
            } else {
                // EventBuffer wrapping case
                println!("Wrapping EventBuffer..");
                // println!("start {}", params_ref.curr_time_start);
                // println!("event {}", next_event.time);
                // println!("end {}", params_ref.curr_time_end);
                seq_int.j_window_time_start <= next_event.time
                    || next_event.time < seq_int.j_window_time_end
            };

            // We let the seq play once through all midi off notes when stopping.
            if halting {
                // Let through all midi off msgs for Pause and stop.
                if let EventType::MidiNote(n) = next_event.e_type {
                    if !n.on_off {
                        push_event = true;
                    }
                }
            }

            if push_event {
                match next_event.e_type {
                    EventType::MidiNote(ref note) => {
                        let raw_midi = RawMidi {
                            //TODO add some frames here for precise timing, as a process cycle is 42ms, see jack doc
                            // This should allow to map events on specific frames, making the above if condition redundant
                            time: ps.frames_since_cycle_start(),
                            bytes: &note.get_raw_note_on_bytes(),
                        };
                        // Max event buff size was measured at ~32kbits ? In practice, 800-2200 midi msgs
                        out_buff.write(&raw_midi).unwrap();
                        println!(
                            "Sending midi note: Channel {:<5} Pitch {:<5} Vel {:<5} On/Off {:<5}",
                            note.channel, note.pitch, note.velocity, note.on_off
                        );
                    }
                }
                seq_int.event_head = (seq_int.event_head + 1) % event_buffer.len();
            //TODO clean up below
            } else if halting {
                seq_int.event_head = (seq_int.event_head + 1) % event_buffer.len();
                if seq_int.event_head != event_head_before {
                    continue;
                };
            } else {
                break;
            }
            if halting && seq_int.event_head == event_head_before {
                break;
            }
        }

        // Reset the seq in case of a stop or pause
        if seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop {
            seq_int.event_head = event_head_before;
            seq_int.status = SeqInternalStatus::Silence;
        }
        if seq_params.status == SeqStatus::Stop {
            seq_int.stop_reset();
        }

        seq_int.j_window_time_start = seq_int.j_window_time_end;
        // println!("frames sunce start {}", ps.frames_since_cycle_start());

        jack::Control::Continue
    };

    // Start the Jack thread
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Start the OSC listening thread
    let udp_socket = UdpSocket::bind(format!("127.0.0.1:{}", OSC_PORT))?;
    let osc_process = osc_process_closure(udp_socket, params_arc);
    let osc_handler = thread::spawn(osc_process);

    // Wait for user input to quit
    println!("Press enter/return to quit...");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
    active_client.deactivate().unwrap();
    let osc_res = osc_handler.join();
    println!("OSC shutdown: {:?}", osc_res);

    Ok(())
}
