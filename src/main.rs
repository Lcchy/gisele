use anyhow::Result;
use jack::{Client, ClientOptions};
use osc::{osc_process_closure, OSC_PORT};
use seq::{EventType, SeqStatus};
use std::{io, net::UdpSocket, sync::Arc, thread, time::Duration};

use crate::{jackp::jack_process_closure, seq::Sequencer};

mod jackp;
mod midi;
mod osc;
mod seq;

const INIT_BPM: f32 = 120.;

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let midi_out = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    // Initiate sequencer and build the Jack process
    let seq_arc = Arc::new(Sequencer::new(INIT_BPM));
    let seq_ref = seq_arc.clone();
    let jack_process = jack_process_closure(seq_ref, midi_out);

    // Start the Jack thread
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Start the OSC listening thread
    let udp_socket = UdpSocket::bind(format!("0.0.0.0:{OSC_PORT}"))?;
    // Setting the UDP recv timeout to 1s to allow for gracefull shutdown
    udp_socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    let osc_process = osc_process_closure(udp_socket, seq_arc.clone());
    let osc_handler = thread::spawn(osc_process);

    // Graceful shutdown on user input
    println!("Press enter/return to quit...");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
    seq_arc.params.write().status = SeqStatus::Shutdown;
    active_client.deactivate().unwrap();
    println!("Jack process shutdown.");
    println!("Waiting for OSC process...");
    osc_handler.join().unwrap()?;

    Ok(())
}
