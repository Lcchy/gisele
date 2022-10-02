use std::{net::UdpSocket, str::FromStr, sync::Arc};

use anyhow::bail;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use rosc::OscMessage;

use crate::{Params, SeqStatus};

/// Should be enough,See https://osc-dev.create.ucsb.narkive.com/TyotlluU/osc-udp-packet-sizes-for-interoperability
/// and https://www.music.mcgill.ca/~gary/306/week9/osc.html
// const OSC_BUFFER_LEN: usize = 4096;
const OSC_BUFFER_LEN: usize = rosc::decoder::MTU;
pub const OSC_PORT: &str = "34254";

fn osc_handling(osc_msg: &OscMessage, params: &Arc<Params>) -> anyhow::Result<()> {
    match osc_msg.addr.as_str() {
        "/gisele/set_status" => {
            let status = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC status arg was not recognized."))?;
            let mut seq_params_mut = params.seq_params.write().unwrap();
            seq_params_mut.status = FromPrimitive::from_u32(status as u32)
                .ok_or_else(|| anyhow::format_err!("OSC status arg was not in enum."))?;
            println!("Grain Status set to {:?}", seq_params_mut.status);
        }
        // "/gisele/params" => {
        //     let mut grain_params_mut = params.grain.write().unwrap();
        //     let start = osc_msg.args[0]
        //         .to_owned()
        //         .int()
        //         .ok_or_else(|| anyhow::format_err!("OSC start arg was not recognized."))?;
        //     let len = osc_msg.args[1]
        //         .to_owned()
        //         .int()
        //         .ok_or_else(|| anyhow::format_err!("OSC len arg was not recognized."))?;

        //     if len > XFADE_LEN as i32 {
        //         grain_params_mut.start = min(start as usize, buffer.len);
        //         grain_params_mut.len = min(len as usize, buffer.len);
        //         println!("Grain start set to {:?}", grain_params_mut.start);
        //         println!("Grain len set to {:?}", len);
        //     } else {
        //         println!("OSC len message argument cannot be less than XFADE.");
        //     }
        // }
        _ => bail!("OSC routing was not recognized"),
    }
    Ok(())
}

/// Returns a closure that runs the main osc receiving loop
pub fn osc_process_closure(
    udp_socket: UdpSocket,
    params_ref: Arc<Params>,
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
