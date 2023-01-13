use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use osc::{osc_process_closure, OSC_PORT};
use seq::{EventType, SeqInternal, SeqInternalStatus, SeqStatus};
use std::{io, net::UdpSocket, sync::Arc, thread};

use crate::seq::Sequencer;

mod midi;
mod osc;
mod seq;

const INIT_BPM: u16 = 120;

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let mut out_port = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    // Init values
    let seq_arc = Arc::new(Sequencer::new(INIT_BPM, 16));
    let seq_ref = seq_arc.clone();
    let mut seq_int = SeqInternal::new();

    // Define the Jack process
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let seq_params = seq_ref.params.read().unwrap();

        // Handle Sequencer statuses
        if seq_params.status == SeqStatus::Start {
            seq_int.status = SeqInternalStatus::Playing;
        }
        if seq_int.status == SeqInternalStatus::Silence {
            return jack::Control::Continue;
        }

        let event_buffer = &*seq_ref.event_buffer.read().unwrap();
        let mut out_buff = out_port.writer(ps);
        let loop_len = seq_params.get_loop_len_in_us();
        let cy_times = ps.cycle_times().unwrap();

        // we increment the current jack process cycle time window dynamically to allow speed playback variations
        seq_int.j_window_time_end = (seq_int.j_window_time_end
            + (((cy_times.next_usecs - cy_times.current_usecs) as f64)
                * (seq_params.bpm as f64 / INIT_BPM as f64)) as u64)
            % loop_len;

        // println!("next_event.time {}", next_event.time);
        // println!("Curr time {}", params_ref.curr_time_end);
        // println!("Curr frames {}", cy_times.current_frames);
        // println!("frames sunce start {}", ps.frames_since_cycle_start());

        let event_head_before = seq_int.event_head;
        let halting = seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop;
        while let Some(next_event) = &event_buffer.get(seq_int.event_head) {
            // println!("Next note Time {}", next_event.time);

            let mut push_event = seq_int.event_in_cycle(next_event.time);

            // We let the seq play once through all midi off notes when halting.
            let mut jump_event = false;
            if halting {
                if let EventType::MidiNote(n) = next_event.e_type {
                    if !n.on_off {
                        // Let through all midi off msgs for Pause and stop.
                        push_event = true;
                    } else if (seq_int.event_head + 1) % event_buffer.len() != event_head_before {
                        jump_event = true;
                    }
                }
            };
            jump_event = jump_event || loop_len < next_event.time;

            if jump_event {
                seq_int.event_head = (seq_int.event_head + 1) % event_buffer.len();
            } else if push_event {
                match next_event.e_type {
                    EventType::MidiNote(ref note) => {
                        let raw_midi = RawMidi {
                            time: ps.frames_since_cycle_start(),
                            bytes: &note.get_raw_note_on_bytes(),
                        };
                        // Max event buff size was measured at ~32kbits ? In practice, 800-2200 midi msgs
                        out_buff.write(&raw_midi).unwrap();
                        println!(
                            "Sending midi note: Channel {:<5} Pitch {:<5} Vel {:<5} On/Off {:<5} Note Time {}",
                            note.channel, note.pitch, note.velocity, note.on_off, next_event.time
                        );
                    }
                    EventType::_Fill => todo!(),
                }
                seq_int.event_head = (seq_int.event_head + 1) % event_buffer.len();
            } else {
                // Complete the current cycle when reaching a note to be played in the next one
                break;
            }

            // Stop playing when having completed a whole loop
            if seq_int.event_head == event_head_before {
                break;
            }
        }

        // Reset the seq to start or current position in case of a stop or pause
        if seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop {
            seq_int.event_head = event_head_before;
            seq_int.status = SeqInternalStatus::Silence;
        }
        if seq_params.status == SeqStatus::Stop {
            seq_int.stop_reset();
        }

        seq_int.j_window_time_start = seq_int.j_window_time_end;

        jack::Control::Continue
    };

    // Start the Jack thread
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Start the OSC listening thread
    let udp_socket = UdpSocket::bind(format!("127.0.0.1:{}", OSC_PORT))?;
    let osc_process = osc_process_closure(udp_socket, seq_arc);
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
