//! End-to-end networking smoke test: a real host and a real client on this machine.
//!
//! `cargo test -p giochi-net` covers the same code with finer assertions, but it runs on
//! the build machine. This binary exists so the console can run it: the target's kernel,
//! its socket buffers, its loopback and its `Instant` are all different, and the R35S's
//! glibc is older than the host's, which is why the libtest harness will not even start
//! there (it pulls a `GLIBC_2.39` weak symbol). A plain example links against what the
//! device actually provides.
//!
//! ```text
//! cargo run -p giochi-net --example loopback_smoke
//! # on the console, after cross-building:
//! ./loopback_smoke
//! ```
//!
//! Exit code 0 means every check passed. A failure prints what it expected and returns 1,
//! so it can be used as a gate without parsing the output.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use giochi_net::{Cliente, Estado, Host, Mensagem, SnapshotInimigo, SNAPSHOT_HZ};

/// How long to wait for a packet before declaring the run failed. Loopback is
/// sub-millisecond; 5 s only keeps a hung run from hanging the terminal.
const PRAZO: Duration = Duration::from_secs(5);

fn esperar(mut cond: impl FnMut() -> bool) -> bool {
    let limite = Instant::now() + PRAZO;
    while Instant::now() < limite {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    cond()
}

fn inimigo(id: u16, x: f32, z: f32) -> SnapshotInimigo {
    SnapshotInimigo {
        id,
        x,
        z,
        hp: 10,
        tipo: 1,
        fase_chefe: 0,
    }
}

fn main() -> ExitCode {
    // `Host::novo(None)` means `PORTA_PADRAO`, a fixed port. Port 0 asks the OS instead,
    // which is what the C ABI promises for `giochi_net_host_create(0)` and the only way
    // two of these can run at once.
    let mut host = match Host::escutar(("0.0.0.0", 0)) {
        Ok(host) => host,
        Err(err) => {
            eprintln!("FAIL could not start the host: {err}");
            return ExitCode::FAILURE;
        }
    };
    let endereco: SocketAddr = match host.endereco() {
        Ok(endereco) => endereco,
        Err(err) => {
            eprintln!("FAIL could not read the host address: {err}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "host on port {} (snapshot {} Hz)",
        endereco.port(),
        SNAPSHOT_HZ
    );
    if endereco.port() == 0 {
        eprintln!("FAIL ephemeral port resolved to 0");
        return ExitCode::FAILURE;
    }

    let mut cliente = match Cliente::entrar(("127.0.0.1", endereco.port())) {
        Ok(cliente) => cliente,
        Err(err) => {
            eprintln!("FAIL could not join: {err}");
            return ExitCode::FAILURE;
        }
    };

    // ---- handshake ------------------------------------------------------
    let mut jogador = None;
    let conectou = esperar(|| {
        host.processar(64);
        for _ in cliente.processar(64) {}
        if jogador.is_none() {
            jogador = cliente.jogador();
        }
        jogador.is_some()
    });
    let Some(id) = jogador else {
        eprintln!(
            "FAIL the client never received a player id (state {:?})",
            cliente.estado()
        );
        return ExitCode::FAILURE;
    };
    if !conectou || cliente.estado() != Estado::Conectado(id) {
        eprintln!(
            "FAIL state is {:?}, expected Conectado({id})",
            cliente.estado()
        );
        return ExitCode::FAILURE;
    }
    if id == 0 {
        eprintln!("FAIL the host assigned id 0, which is reserved for the host");
        return ExitCode::FAILURE;
    }
    println!("ok   handshake: player {id}");

    // ---- input reaches the host ----------------------------------------
    let camera_x = 100.0;
    if let Err(err) = cliente.enviar_input(Mensagem::Input {
        tick: 1,
        mover_x: 0.5,
        mover_z: -0.5,
        mirar_x: 1.0,
        mirar_z: 0.0,
        acoes: 0b101,
        camera_x,
        camera_z: 0.0,
    }) {
        eprintln!("FAIL could not send input: {err}");
        return ExitCode::FAILURE;
    }

    let mut input_chegou = false;
    let chegou = esperar(|| {
        for (quem, mensagem) in host.processar(64) {
            if let Mensagem::Input { camera_x: x, .. } = mensagem {
                input_chegou = quem == id && (x - camera_x).abs() < f32::EPSILON;
            }
        }
        input_chegou
    });
    if !chegou {
        eprintln!("FAIL the input did not reach the host intact");
        return ExitCode::FAILURE;
    }
    println!("ok   input host<-client, camera {camera_x}");

    // ---- AOI: only what the client can see ------------------------------
    // The camera is at x=100, so the enemy at x=101 is inside a 50-unit radius and the
    // one at x=900 is not. The distant one must never appear in a snapshot.
    host.definir_area(id, 50.0);
    let perto = inimigo(1, 101.0, 0.0);
    let longe = inimigo(2, 900.0, 0.0);

    // `tick_snapshot` is rate-limited to SNAPSHOT_HZ, so one call is not enough: the
    // first may be swallowed by the interval since the host was created.
    let mut vistos: Vec<u16> = Vec::new();
    let mut recebeu_snapshot = false;
    let mut recebeu_global = false;
    let mut tentativas = 0;
    while tentativas < 40 {
        tentativas += 1;
        if host.tick_snapshot(&[perto, longe]) {
            host.enviar_global(b"onda-3");
        }
        for mensagem in cliente.processar(64) {
            match mensagem {
                Mensagem::Inimigos { inimigos, .. } => {
                    if !inimigos.is_empty() {
                        recebeu_snapshot = true;
                        vistos = inimigos.iter().map(|i| i.id).collect();
                    }
                }
                Mensagem::Global { dados, .. } if dados == b"onda-3" => recebeu_global = true,
                _ => {}
            }
        }
        if recebeu_snapshot && recebeu_global {
            break;
        }
        std::thread::sleep(Duration::from_millis(1000 / SNAPSHOT_HZ as u64));
    }

    if !recebeu_snapshot {
        eprintln!("FAIL no enemy snapshot arrived");
        return ExitCode::FAILURE;
    }
    if vistos != vec![perto.id] {
        eprintln!(
            "FAIL AOI sent {vistos:?}; expected only enemy {} (the one at x=900 is 800 units away)",
            perto.id
        );
        return ExitCode::FAILURE;
    }
    println!("ok   AOI sent only the enemy inside the radius ({vistos:?})");

    if !recebeu_global {
        eprintln!("FAIL the global state never arrived");
        return ExitCode::FAILURE;
    }
    println!("ok   global state host->client ({} bytes)", b"onda-3".len());

    // ---- counters -------------------------------------------------------
    let (recebidos, invalidos, enviados) = cliente.contadores();
    if recebidos == 0 || invalidos != 0 || enviados == 0 {
        eprintln!(
            "FAIL counters look wrong: received {recebidos}, invalid {invalidos}, sent {enviados}"
        );
        return ExitCode::FAILURE;
    }
    println!("ok   counters received={recebidos} invalid={invalidos} sent={enviados}");

    println!("loopback_smoke: OK");
    ExitCode::SUCCESS
}
