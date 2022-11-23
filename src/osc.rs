use std::{net::UdpSocket, sync::Arc};

use anyhow::bail;
use num_traits::FromPrimitive;
use rosc::OscMessage;

use crate::{midi::midi_pitch_to_note, Sequencer};

/// Should be enough,See https://osc-dev.create.ucsb.narkive.com/TyotlluU/osc-udp-packet-sizes-for-interoperability
/// and https://www.music.mcgill.ca/~gary/306/week9/osc.html
// const OSC_BUFFER_LEN: usize = 4096;
const OSC_BUFFER_LEN: usize = rosc::decoder::MTU;
pub const OSC_PORT: &str = "34254";

fn osc_handling(osc_msg: &OscMessage, seq: &Arc<Sequencer>) -> anyhow::Result<()> {
    match osc_msg.addr.as_str() {
        "/gisele/set_status" => {
            let status = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC status arg was not recognized."))?;
            let mut seq_params_mut = seq.params.write().unwrap();
            seq_params_mut.status = FromPrimitive::from_u32(status as u32)
                .ok_or_else(|| anyhow::format_err!("OSC status arg was not in enum."))?;
            println!("Sequencer Status set to {:?}", seq_params_mut.status);
        }
        "/gisele/set_bpm" => {
            let mut params_mut = seq.params.write().unwrap();
            params_mut.bpm = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC bpm arg wass not recognized."))?
                as u16;
        }
        "/gisele/set_loop_length" => {
            let mut params_mut = seq.params.write().unwrap();
            params_mut.loop_length = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC loop_len arg was not recognized."))?
                as u64;
        }
        "/gisele/set_loop_length_bars" => {
            let mut params_mut = seq.params.write().unwrap();
            params_mut.loop_length = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC loop_len arg was not recognized."))?
                as u64;
            //TODO
        }
        "/gisele/set_nb_events" => {
            let mut params_mut = seq.params.write().unwrap();
            params_mut.nb_events = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC nb_events arg was not recognized."))?
                as u64;
            seq.reseed()
        }
        "/gisele/set_root" => {
            let mut params_mut = seq.params.write().unwrap();
            params_mut.root_note =
                midi_pitch_to_note(
                    osc_msg.args[0].to_owned().int().ok_or_else(|| {
                        anyhow::format_err!("OSC root_note arg was not recognized.")
                    })? as u8,
                );
            seq.reseed()
        }
        "/gisele/set_note_len" => {
            let mut params_mut = seq.params.write().unwrap();
            params_mut.note_length = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC root_note arg was not recognized."))?
                as u8;
            seq.reseed()
        }
        "/gisele/reseed" => {
            println!("Reseeding..");
            seq.reseed();
            println!("Finished reseeding");
        }
        _ => bail!("OSC routing was not recognized"),
    }
    Ok(())
}

/// Returns a closure that runs the main osc receiving loop
pub fn osc_process_closure(
    udp_socket: UdpSocket,
    params_ref: Arc<Sequencer>,
) -> impl FnOnce() -> anyhow::Result<()> {
    move || {
        let mut rec_buffer = [0; OSC_BUFFER_LEN];
        loop {
            match udp_socket.recv(&mut rec_buffer) {
                Ok(received) => {
                    let (_, packet) =
                        if let Ok(v) = rosc::decoder::decode_udp(&rec_buffer[..received]) {
                            v
                        } else {
                            println!("OSC message could not be decoded.");
                            continue;
                        };
                    match packet {
                        rosc::OscPacket::Message(msg) => {
                            println!("Received osc msg {:?}", msg);
                            let r = osc_handling(&msg, &params_ref);
                            if let Err(e) = r {
                                println!("OSC message handling failed with: {:?}", e);
                            }
                        }
                        rosc::OscPacket::Bundle(_) => unimplemented!(),
                    }
                }
                Err(e) => println!("recv function failed: {:?}", e),
            }
        }
    }
}
