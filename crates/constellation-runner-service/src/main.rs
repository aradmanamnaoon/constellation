//! Local operator CLI. Default stays stdio-only; explicit loopback TCP is opt-in.
use std::{
    env, fs,
    fs::OpenOptions,
    io::{self, Write},
    net::TcpListener,
    path::Path,
};

use constellation_runner_protocol::{
    read_frame, write_frame, Capability, PermissionScope, TaskAttribution,
};
use constellation_runner_service::{serve_loopback, RunnerStore, SystemClock};
use zeroize::Zeroizing;

fn read_private_key(path: &Path) -> Result<[u8; 32], ()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(());
        }
    }
    let data = Zeroizing::new(fs::read_to_string(path).map_err(|_| ())?);
    let text = data.trim_end_matches(['\n', '\r']);
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    let mut secret = [0_u8; 32];
    for (i, byte) in secret.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| ())?;
    }
    if secret.iter().all(|byte| *byte == 0) {
        return Err(());
    }
    Ok(secret)
}

fn generate_key(path: &Path) -> Result<(), ()> {
    let mut secret = Zeroizing::new([0_u8; 32]);
    getrandom::fill(&mut *secret).map_err(|_| ())?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| ())?;
    for byte in secret.iter() {
        write!(file, "{byte:02x}").map_err(|_| ())?;
    }
    file.write_all(b"\n").map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    Ok(())
}

fn capability(raw: &str) -> Result<Capability, ()> {
    match raw {
        "read_evidence" => Ok(Capability::ReadEvidence),
        "draft_text" => Ok(Capability::DraftText),
        "analyze_data" => Ok(Capability::AnalyzeData),
        _ => Err(()),
    }
}

fn serve(mut store: RunnerStore) -> Result<(), ()> {
    if store.approval_status().map_err(|_| ())? != "approved" {
        return Err(());
    }
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    // Connection is entirely local stdio. An integration must add a secure,
    // authenticated and encrypted transport, with rate and time limits.
    for _ in 0..1024 {
        let Some(frame) = read_frame(&mut input).map_err(|_| ())? else {
            return Ok(());
        };
        let receipt = store.accept(&frame, &SystemClock).map_err(|_| ())?;
        write_frame(&mut output, &receipt).map_err(|_| ())?;
    }
    Err(()) // Re-enroll/rotate before unbounded message tracking.
}

fn command(args: &[String]) -> Result<(), ()> {
    match args {
        [_, action, path] if action == "generate-key" => {
            generate_key(Path::new(path))?;
            println!(
                "Created private key file. Exchange it only through a secure out-of-band channel."
            );
        }
        [_, action, db, key, workstation, runner] if action == "init" => {
            RunnerStore::initialize(
                Path::new(db),
                read_private_key(Path::new(key))?,
                workstation,
                runner,
            )
            .map_err(|_| ())?;
            println!("Runner pair pending. Run approve-pair locally after verifying identities.");
        }
        [_, action, db, key] if action == "approve-pair" => {
            RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?
                .approve_pair()
                .map_err(|_| ())?;
            println!("Locally approved pair.");
        }
        [_, action, db, key] if action == "revoke-pair" => {
            RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?
                .revoke_pair()
                .map_err(|_| ())?;
            println!("Pair permanently revoked; enroll fresh key and database.");
        }
        [_, action, db, key] if action == "status" => {
            let store = RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?;
            println!(
                "pair={} inbox_jobs={}",
                store.approval_status().map_err(|_| ())?,
                store.list_jobs().map_err(|_| ())?.len()
            );
        }
        [_, action, db, key] if action == "list" => {
            let store = RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?;
            for job in store.list_jobs().map_err(|_| ())? {
                if let Some(task) = job.task {
                    println!(
                        "job={} agent={} owner={} run={} task={} capability={:?} resource={} state={:?}",
                        job.job_id, job.agent_id, task.owner_id, task.run_id, task.task_id,
                        job.scope.capability, job.scope.resource_id, job.state
                    );
                } else {
                    println!(
                        "job={} agent={} capability={:?} resource={} state={:?}",
                        job.job_id,
                        job.agent_id,
                        job.scope.capability,
                        job.scope.resource_id,
                        job.state
                    );
                }
            }
        }
        [_, action, db, key, job, capability_name, resource] if action == "grant" => {
            let mut store = RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?;
            store
                .grant_job(
                    job,
                    &PermissionScope {
                        capability: capability(capability_name)?,
                        resource_id: resource.clone(),
                    },
                )
                .map_err(|_| ())?;
            println!("Local grant recorded for exact job and resource; no tool executed.");
        }
        [_, action, db, key, job, owner, run, task, capability_name, resource]
            if action == "grant-bound" =>
        {
            RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?
                .grant_bound_job(
                    job,
                    &TaskAttribution {
                        owner_id: owner.clone(),
                        run_id: run.clone(),
                        task_id: task.clone(),
                    },
                    &PermissionScope {
                        capability: capability(capability_name)?,
                        resource_id: resource.clone(),
                    },
                )
                .map_err(|_| ())?;
            println!("Exact owner/run/task and resource grant recorded; no tool executed.");
        }
        [_, action, db, key, job] if action == "revoke-grant" => {
            RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?
                .revoke_grant(job)
                .map_err(|_| ())?;
            println!("Local grant revoked.");
        }
        [_, action, db, key] if action == "serve-stdio" => {
            let store = RunnerStore::open(Path::new(db), read_private_key(Path::new(key))?)
                .map_err(|_| ())?;
            serve(store)?;
        }
        [_, action, db, key, port] if action == "serve-loopback" => {
            let port = port.parse::<u16>().map_err(|_| ())?;
            if port == 0 {
                return Err(());
            }
            let secret = read_private_key(Path::new(key))?;
            // Check approval/key/schema before binding a socket. The program
            // never accepts a configurable public/LAN bind address.
            if RunnerStore::open(Path::new(db), secret)
                .map_err(|_| ())?
                .approval_status()
                .map_err(|_| ())?
                != "approved"
            {
                return Err(());
            }
            let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|_| ())?;
            serve_loopback(listener, Path::new(db), secret, 1_024).map_err(|_| ())?;
        }
        _ => return Err(()),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if command(&args).is_err() {
        // No credentials, signed wire payload or private instruction is
        // included in diagnostic output (even on invalid frames).
        eprintln!("Runner command rejected. Usage: generate-key KEYFILE | init DB KEYFILE WORKSTATION RUNNER | approve-pair DB KEYFILE | revoke-pair DB KEYFILE | status DB KEYFILE | list DB KEYFILE | grant DB KEYFILE JOB CAPABILITY RESOURCE | grant-bound DB KEYFILE JOB OWNER RUN TASK CAPABILITY RESOURCE | revoke-grant DB KEYFILE JOB | serve-stdio DB KEYFILE | serve-loopback DB KEYFILE PORT (127.0.0.1 only, explicit opt-in)");
        std::process::exit(1);
    }
}
