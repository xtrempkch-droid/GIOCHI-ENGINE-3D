//! Announces a host over mDNS and discovers it, on a real network.
//!
//! The `#[ignore]`d unit test in `descobrir` does this on one machine. This binary does it
//! across two, which is the claim that actually matters: mDNS sends to the multicast group
//! **on a specific interface**, and a `send_to` without `IP_MULTICAST_IF` leaves by the
//! default route and is seen by nobody. That failure looks exactly like "the other device
//! is not there".
//!
//! ```text
//! # host announces, console discovers (or the other way round)
//! cargo run -p giochi-net --features mdns --example mdns_smoke -- announce 47721
//! cargo run -p giochi-net --features mdns --example mdns_smoke -- discover 30
//! ```
//!
//! On a multi-homed host the default route may not be the interface the console is on, so
//! `announce` accepts `--ip` to pin the address:
//!
//! ```text
//! # host has a default route through Wi-Fi, console on a USB-Ethernet at 10.55.0.1
//! ... --example mdns_smoke -- announce 47721 60 --ip 10.55.0.1
//! ```
//!
//! Exit code 0 means the service was found. It is built only with the `mdns` feature,
//! because that is what pulls `mdns-sd` in.
#![cfg(feature = "mdns")]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use giochi_net::{Anunciante, Descoberta};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("announce") => {
            let porta: u16 = args
                .get(1)
                .and_then(|value| value.parse().ok())
                .unwrap_or(47721);

            let forcado = args
                .iter()
                .position(|arg| arg == "--ip")
                .and_then(|index| args.get(index + 1))
                .map(|value| value.parse::<std::net::IpAddr>());

            let anunciado = match forcado {
                None => Anunciante::anunciar(porta),
                Some(Ok(ip)) => {
                    println!("pinning the announcement to {ip}");
                    Anunciante::anunciar_em(ip, porta)
                }
                Some(Err(err)) => {
                    eprintln!("FAIL --ip is not an address: {err}");
                    return ExitCode::FAILURE;
                }
            };

            match anunciado {
                Ok(anunciante) => {
                    println!("announcing '{}' on port {porta}", anunciante.nome());
                    // The daemon runs on its own thread; the process must stay alive for
                    // the record to stay in the cache of the other side.
                    let segundos: u64 = args
                        .get(2)
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(45);
                    println!("staying up for {segundos}s");
                    std::thread::sleep(Duration::from_secs(segundos));
                    anunciante.encerrar();
                    println!("mdns_smoke: OK (announced)");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("FAIL could not announce: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("discover") => {
            let segundos: u64 = args
                .get(1)
                .and_then(|value| value.parse().ok())
                .unwrap_or(30);

            let descoberta = match Descoberta::iniciar() {
                Ok(descoberta) => descoberta,
                Err(err) => {
                    eprintln!("FAIL could not start the browse: {err}");
                    return ExitCode::FAILURE;
                }
            };

            let limite = Instant::now() + Duration::from_secs(segundos);
            let mut achados = Vec::new();
            while Instant::now() < limite {
                for encontrado in descoberta.drenar(16) {
                    println!("found {}", encontrado.endereco);
                    achados.push(encontrado);
                }
                if !achados.is_empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            descoberta.encerrar();

            if achados.is_empty() {
                eprintln!("FAIL nothing was discovered in {segundos}s");
                ExitCode::FAILURE
            } else {
                println!("mdns_smoke: OK ({} found)", achados.len());
                ExitCode::SUCCESS
            }
        }
        _ => {
            eprintln!(
                "usage: mdns_smoke announce [port] [seconds] [--ip ADDR] | discover [seconds]"
            );
            ExitCode::FAILURE
        }
    }
}
