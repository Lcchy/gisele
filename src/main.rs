use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use std::io;

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let mut out_port = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    // Define the Jack process (to refactor)
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let mut out_buff = out_port.writer(ps);
        // out_buff.max_event_size()

        // MIDI 90 3C 40 : Ch1 Note on P60 V64
        let note_on = RawMidi {
            time: ps.frames_since_cycle_start(),
            bytes: &[144, 60, 64],
        };

        // MIDI 80 3C 40 : Ch1 Note off P60 V64  | vel is arbitrary
        let note_off = RawMidi {
            time: ps.frames_since_cycle_start(),
            bytes: &[128, 60, 64],
        };

        out_buff.write(&note_on).unwrap();

        jack::Control::Continue
    };

    // Start the Jack thread/usr/share/codium/resources/app/out/vs/code/electron-sandbox/workbench/workbench.html
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Wait for user input to quit
    println!("Press enter/return to quit...");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
    active_client.deactivate().unwrap();

    Ok(())
}
