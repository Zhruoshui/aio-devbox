// Long-running job execution (sandbox-mgr Phase 1, design §3.4/§3.6).
//
// Creation (and env-change recreate) are minute-scale (image builds), so
// they run as detached tokio tasks: the API returns a job id immediately,
// mgr-web polls GET /api/jobs/:id. Job state lives in the AppState jobs map
// (in-memory, O(1) poll) mirrored into the jobs table (durable; orphaned
// running jobs are marked error on mgr restart - db::fail_orphan_jobs).
//
// The build order is base -> app -> code-server -> vnc (design §3.6): app
// and code-server FROM the base image via --build-arg BASE_IMAGE; vnc is
// env-independent (shared tag) and only built when missing.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{Context, Result};
use tokio::sync::{mpsc, Mutex as TokioMutex};

use crate::caddy;
use crate::composegen;
use crate::db;
use crate::docker;
use crate::envhash;
use crate::state::{AppState, JobShared};

/// Log tail cap for the in-memory job record. The progress view's live
/// window; R3 raised it from 8KB to 64KB so the 1.5s poll shows more of a
/// running build before the user opens the full on-disk log.
const LOG_TAIL: usize = 64 * 1024;

/// Images-table build_log tail cap (R4.2, R3: 256KB -> 4MB). The column is
/// now an index / fast preview of the log: the FULL output lives on disk in
/// `<data>/jobs/<job_id>.log` and is served by GET /api/jobs/:id/log.
const BUILD_LOG_TAIL: usize = 4 * 1024 * 1024;

/// Spawn the create/recreate job for a sandbox. All parameters are owned -
/// the task outlives the request. Returns the job id.
pub async fn spawn_create(
    state: Arc<AppState>,
    name: String,
    env: envhash::SandboxEnv,
    cpus: Option<f64>,
    mem_mb: Option<i64>,
    recreate: bool,
    services_json: String,
) -> Result<i64> {
    // Durable job row first so a crash between spawn and first update still
    // leaves a traceable job.
    let job_id = {
        let conn = state.db.lock().unwrap();
        db::insert_job(
            &conn,
            if recreate { "recreate" } else { "create" },
            Some(&name),
        )?
    };

    let shared = Arc::new(TokioMutex::new(JobShared {
        id: job_id,
        kind: if recreate { "recreate" } else { "create" }.into(),
        sandbox: Some(name.clone()),
        status: "running".into(),
        error: None,
        log: String::new(),
        // R3: filled in by run_create as soon as the full-log file is open.
        log_path: None,
    }));
    {
        let mut jobs = state.jobs.lock().unwrap();
        jobs.insert(job_id, shared.clone());
    }

    let st = state.clone();
    tokio::spawn(async move {
        let result = run_create(
            st.clone(),
            name.clone(),
            env,
            cpus,
            mem_mb,
            recreate,
            services_json,
            shared.clone(),
        )
        .await;
        let mut job = shared.lock().await;
        match result {
            Ok(()) => {
                job.status = "ok".into();
                let _ = db::update_sandbox_status(&st.db.lock().unwrap(), &name, "running");
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::error!(sandbox = %name, error = %msg, "create job failed");
                job.status = "error".into();
                job.error = Some(msg);
                let _ = db::update_sandbox_status(&st.db.lock().unwrap(), &name, "error");
            }
        }
        let _ = db::persist_job(&st.db.lock().unwrap(), &job);
    });

    Ok(job_id)
}

// The create/recreate job's full parameter list mirrors spawn_create's API
// surface one-to-one; bundling into a struct would only reshuffle the same
// eight values behind a new name.
#[allow(clippy::too_many_arguments)]
async fn run_create(
    state: Arc<AppState>,
    name: String,
    env: envhash::SandboxEnv,
    cpus: Option<f64>,
    mem_mb: Option<i64>,
    recreate: bool,
    services_json: String,
    log: Arc<TokioMutex<JobShared>>,
) -> Result<()> {
    let repo = state.repo.clone();
    let instance = state.instance_dir(&name);
    std::fs::create_dir_all(&instance).with_context(|| format!("create {}", instance.display()))?;
    // S1: the create-time service set drives image builds (code-server), the
    // compose file's service blocks, and the up profiles (vnc). Parsed once;
    // invalid values (hand-rolled job replay) fall back to all-on.
    let services: db::Services = serde_json::from_str(&services_json).unwrap_or_default();

    // R3: this is the job kind whose output is worth keeping WHOLE (image
    // builds run to tens of MB). Open the full-log file up front, publish its
    // path on the job cell + the jobs row, and route every subsequent append
    // through `log`. A failure to open degrades to memory-only (warn, build
    // continues): the disk copy is observability, never a build dependency.
    let job_id = log.lock().await.id;
    let log = match open_job_log(&state, job_id) {
        Ok((path, file)) => {
            let path = path.display().to_string();
            {
                let mut cell = log.lock().await;
                cell.log_path = Some(path.clone());
            }
            // Record the path BEFORE the build starts: the image-log endpoint
            // reads it from the jobs table, and it may be called after an mgr
            // restart when the in-memory job map is gone. A failed row write
            // is not fatal (the in-memory cell still carries the path for the
            // life of this process) - warn and carry on.
            if let Err(e) = db::set_job_log_path(&state.db.lock().unwrap(), job_id, &path) {
                tracing::warn!(job = job_id, error = %e, "job log path not persisted");
            }
            JobLog::with_file(log.clone(), file)
        }
        Err(e) => {
            tracing::warn!(job = job_id, error = %e, "full job log unavailable, memory tail only");
            JobLog::memory(log.clone())
        }
    };

    // 1. env -> assembled Dockerfile.base content (L1-L4 via gen + L5 custom
    //    tools via assemble_with_tools) -> env_hash. Validation happens here
    //    (not just in the API) so a job replay from a stale DB still fails
    //    safely; assemble_with_tools re-validates through to_manifest_checked.
    let dockerfile_base = envhash::assemble_with_tools(&repo, &env)?;
    let hash = envhash::env_hash(&dockerfile_base);
    let (base_tag, app_tag, cs_tag) = envhash::image_tags(&hash);

    append_log(
        &log,
        &format!("env_hash {hash}\nbase assembly ok, images: {base_tag} / {app_tag} / {cs_tag}\n"),
    )
    .await;

    // 2. Shared network (idempotent) - required by the generated compose.
    docker::ensure_network("aio-mgr-net").await?;
    append_log(&log, "network aio-mgr-net ok\n").await;

    // 3. Images: build missing ones. Order base -> app -> code-server -> vnc
    //    (dependency: app/cs FROM base). Same-env reuse (A5) hits the
    //    existence checks and skips everything. Builds STREAM their output
    //    into the job log line by line (R4.1); the base build's full captured
    //    output is kept (tail) as the images-table build_log - the registry
    //    page's "build log" column / failure display (design §3.3).
    let mut base_build_log = String::new();
    // Line-forwarding for the streaming builds (R4.1): every docker build
    // line goes into the job log as it arrives. ONE ordered drainer task
    // consumes an mpsc channel, so the log keeps the build's emission order -
    // a tokio::spawn per line is NOT a valid shortcut: the multi-threaded
    // runtime schedules them in arbitrary order and the reproduced result was
    // visibly shuffled BuildKit output (verified: 43k-line burst, lines
    // swapped within the first handful).
    //
    // The channel is UNBOUNDED: the callback runs on a runtime worker thread
    // (docker::build's read loop), where the bounded `blocking_send` panics
    // with "cannot block the current thread from within a runtime" - the
    // unbounded `send` is a plain non-blocking call. Backpressure is not
    // needed: the drain task only locks + pushes into a 64KB-capped buffer
    // plus one write syscall, and a build's whole output is a few MB at most.
    //
    // R3: the drain task is the ONLY writer from here on, for run_create's own
    // lines as much as for docker build's. The drain is ASYNC and can lag
    // thousands of lines behind the (fast) pipe reader, so a direct
    // `append_log` from this task would interleave with - or land BEFORE -
    // the still-queued build output: the images page would then open a
    // finished image's log and find "sandbox running" in the middle and the
    // build's tail missing. Sending everything through the one ordered queue
    // removes the race; `flush_chunks` is the barrier that makes the file
    // complete before the job is marked settled (and before the image row
    // that points at it is written).
    //
    // Dropping the senders (end of run_create) closes the channel and makes
    // the drain task finish on its own.
    let (chunk_tx, mut log_rx) = mpsc::unbounded_channel::<LogMsg>();
    let drain_log = log.clone();
    tokio::spawn(async move {
        while let Some(msg) = log_rx.recv().await {
            match msg {
                LogMsg::Line(chunk) => append_log(&drain_log, &chunk).await,
                // The barrier: everything sent before it is now written.
                LogMsg::Flush(ack) => {
                    let _ = ack.send(());
                }
            }
        }
    });
    // The docker-build callback's handle (it runs on the runtime's worker
    // thread - the `chunk_tx` above is run_create's own).
    let build_tx = chunk_tx.clone();
    let mut line_forward = move |line: &str| {
        // Closed channel (runtime shutting down) -> drop the line rather than
        // fail the whole create job.
        send_chunk(&build_tx, format!("{line}\n"));
    };
    if !docker::image_exists(&base_tag).await? {
        send_chunk(&chunk_tx, format!("building {base_tag} ...\n"));
        // Per-sandbox Dockerfile.base so builds don't share the repo-root
        // output (D2: everything a sandbox needs lives in its instance dir).
        let df_path = instance.join("Dockerfile.base");
        std::fs::write(&df_path, &dockerfile_base)
            .with_context(|| format!("write {}", df_path.display()))?;
        match docker::build(&repo, &df_path, &base_tag, &[], &mut line_forward).await {
            Ok(out) => base_build_log = tail(&out, BUILD_LOG_TAIL),
            Err(e) => {
                // R4.3: docker build's failure tail (stdout + stderr merged)
                // goes back into the job log - the caller previously buried
                // it inside the anyhow error only. The log is prefixed so the
                // 2KB cap on that tail (docker.rs tail_str) can never hide
                // the line itself when the job log tail is read.
                send_chunk(&chunk_tx, format!("=== build failed: {e:#}\n"));
                // R3: drain the queued build output before the failure row
                // (which points at this job's log) is written - the images
                // page must find the whole failure, not the part the drain
                // task got to. This is the early-return path, so the barrier
                // has to run explicitly rather than at the end of the
                // function.
                flush_chunks(&chunk_tx).await;
                // Record a FAILED-build row so the registry page keeps the
                // failure output for debugging (a failed build previously
                // left no trace outside the ephemeral job log). built_at
                // becomes the attempt time - the row's size reads "—" and
                // refcount stays 0, so cleanup/delete treat it normally.
                {
                    let conn = state.db.lock().unwrap();
                    let combo = envhash::describe_combo(&env, &services);
                    let fail_log = tail(&format!("{e:#}\n"), BUILD_LOG_TAIL);
                    let _ = db::upsert_image(
                        &conn,
                        &hash,
                        &base_tag,
                        &fail_log,
                        Some(&combo),
                        &env.canonical_json(),
                        &services.canonical_json(),
                        // R3: the row points at this job's full log, so the
                        // images page can replay the WHOLE failure, not just
                        // the 4MB column tail.
                        Some(job_id),
                    );
                }
                return Err(e);
            }
        }
    } else {
        send_chunk(&chunk_tx, format!("{base_tag} exists, skip\n"));
    }
    // S1 build switches: app is ALWAYS built (it IS the sandbox);
    // code-server's image only when the sandbox has the service (cs_tag is
    // otherwise dead weight — 4.5GB per combo); vnc's image is GLOBAL (one
    // sandbox-vnc shared by every sandbox) and keyed on the shared tag, so
    // it is built below only when THIS sandbox has vnc and the shared image
    // is missing — later vnc sandboxes hit the image_exists short-circuit.
    if !docker::image_exists(&app_tag).await? {
        send_chunk(&chunk_tx, format!("building {app_tag} ...\n"));
        docker::build(
            &repo,
            &repo.join("app/Dockerfile"),
            &app_tag,
            &[("BASE_IMAGE", &base_tag)],
            &mut line_forward,
        )
        .await?;
    } else {
        send_chunk(&chunk_tx, format!("{app_tag} exists, skip\n"));
    }
    if services.code_server && !docker::image_exists(&cs_tag).await? {
        send_chunk(&chunk_tx, format!("building {cs_tag} ...\n"));
        docker::build(
            &repo,
            &repo.join("code-server/Dockerfile"),
            &cs_tag,
            &[("BASE_IMAGE", &base_tag)],
            &mut line_forward,
        )
        .await?;
    } else if services.code_server {
        send_chunk(&chunk_tx, format!("{cs_tag} exists, skip\n"));
    } else {
        send_chunk(&chunk_tx, "code-server not installed, skip image\n");
    }
    // vnc (see the build-switch comment above): global image, built only
    // when THIS sandbox has the vnc service and the shared tag is missing.
    if services.vnc && !docker::image_exists(envhash::VNC_TAG).await? {
        send_chunk(&chunk_tx, format!("building {} ...\n", envhash::VNC_TAG));
        docker::build(
            &repo,
            &repo.join("vnc/Dockerfile"),
            envhash::VNC_TAG,
            &[],
            &mut line_forward,
        )
        .await?;
    }
    if !services.vnc {
        send_chunk(&chunk_tx, "vnc not installed, skip vnc image\n");
    }

    // R3: drain the queued build output BEFORE the row that points this image
    // at job_id's log file is written - the images page can be opened the
    // instant the create job reports done, and it must find the whole build
    // (not just the part the drain task got to).
    flush_chunks(&chunk_tx).await;
    {
        let conn = state.db.lock().unwrap();
        let combo = envhash::describe_combo(&env, &services);
        db::upsert_image(
            &conn,
            &hash,
            &base_tag,
            &base_build_log,
            Some(&combo),
            &env.canonical_json(),
            &services.canonical_json(),
            // R3: the original build job's full log. A same-env reuse
            // (base_build_log empty) keeps the PRIOR row's job_id (db.rs
            // CASE WHEN) - this job only logged "exists, skip".
            Some(job_id),
        )
        .with_context(|| "record image")?;
    }

    // 4. Compose + Caddyfile.
    let gen = composegen::generate(&name, &hash, cpus, mem_mb, services)?;
    composegen::write(&instance, &gen)?;
    send_chunk(&chunk_tx, "compose.yml + gateway/Caddyfile written\n");

    // 5. up -d (force-recreate on env change keeps volumes - design §3.6).
    //    D4+D1: up carries the vnc profile only when the sandbox has vnc
    //    (docker.rs up_profiles - S1 per-sandbox switch), so the
    //    force-recreate below REPLACES the app container without touching a
    //    leftover code-server container from an earlier on-demand start -
    //    which would keep "running" attached to the REMOVED app's netns
    //    (network_mode: service:app): an unreachable zombie that also breaks
    //    the next full-profile restart ("joining network namespace ... No
    //    such container", verified live). Remove it first - idempotent when
    //    no container exists (fresh create never has one); best-effort, since
    //    a failed rm only leaves the self-healing zombie that the next
    //    code-server pane start repairs (compose recreates stale services).
    //    S1: skipped for code-server-less sandboxes — their compose never
    //    carried the service block, so no zombie can exist (and `rm` against
    //    a service absent from the file is a guaranteed noisy error).
    let project = envhash::project_name(&name);
    let compose_file = instance.join("compose.yml");
    if recreate && services.code_server {
        match docker::compose_service_rm(
            &project,
            &compose_file,
            &docker::CODE_SERVER_PROFILE,
            "code-server",
        )
        .await
        {
            Ok(out) => send_chunk(&chunk_tx, out),
            Err(e) => {
                send_chunk(
                    &chunk_tx,
                    format!("code-server pre-clean failed (continuing): {e:#}\n"),
                )
            }
        }
    }
    let out = docker::compose_up(&project, &compose_file, recreate, services.vnc).await?;
    send_chunk(&chunk_tx, out);

    // 6. Persist the env/config on success (A5 correctness: a failed create
    //    doesn't overwrite a working sandbox's config). Same statement for
    //    create and recreate - recreate already overwrote the row's status in
    //    put_sandbox, and both flows own the row from here on.
    {
        let conn = state.db.lock().unwrap();
        db::update_sandbox_config(
            &conn,
            &name,
            &env.canonical_json(),
            &hash,
            cpus,
            mem_mb,
            &services_json,
        )?;
    }

    // 7. Total gateway: regenerate the Caddyfile with this sandbox's site
    //    pair + reload (Phase 2). Reported into the job log on both paths;
    //    a reload failure keeps the .bak and does not fail the job.
    match caddy::regenerate(&state).await {
        Ok(line) => send_chunk(&chunk_tx, format!("{line}\n")),
        Err(e) => send_chunk(&chunk_tx, format!("gateway regenerate FAILED: {e:#}\n")),
    }

    send_chunk(&chunk_tx, "sandbox running\n");
    flush_chunks(&chunk_tx).await;
    Ok(())
}

/// Delete job: compose down [-v] + row removal. Fast enough to be inline in
/// the API handler, but a job keeps the same progress UX for slow volumes.
pub async fn spawn_delete(state: Arc<AppState>, name: String, volumes: bool) -> Result<i64> {
    let job_id = {
        let conn = state.db.lock().unwrap();
        db::insert_job(&conn, "delete", Some(&name))?
    };
    let shared = Arc::new(TokioMutex::new(JobShared {
        id: job_id,
        kind: "delete".into(),
        sandbox: Some(name.clone()),
        status: "running".into(),
        error: None,
        log: String::new(),
        // Short job: compose down output fits the memory tail, no on-disk log.
        log_path: None,
    }));
    {
        let mut jobs = state.jobs.lock().unwrap();
        jobs.insert(job_id, shared.clone());
    }

    let st = state.clone();
    tokio::spawn(async move {
        let log = JobLog::memory(shared.clone());
        let result = async {
            let compose_file = st.instance_dir(&name).join("compose.yml");
            let project = envhash::project_name(&name);
            // A failed create never wrote a compose file - compose down would
            // error on the missing file and leave an undeletable error row.
            // Skip to the row/dir cleanup instead (nothing is running).
            let out = if compose_file.exists() {
                docker::compose_down(&project, &compose_file, volumes).await?
            } else {
                format!("no compose file for {name} (create failed early) - skip compose down\n")
            };
            append_log(&log, &out).await;
            {
                let conn = st.db.lock().unwrap();
                db::delete_sandbox(&conn, &name)?;
                // Drop the model-profile assignment too (D8): a lingering
                // entry would be silently inherited by a future sandbox
                // created under the same name. Best-effort — a failure here
                // must not fail the teardown (log-and-continue).
                if let Err(e) = crate::models::set_assignment(&conn, &name, None, None) {
                    tracing::warn!(sandbox = %name, error = %e.message, "assignment cleanup failed");
                }
            }
            // Remove the instance dir (generated artifacts, D2). Compose down
            // already took containers/networks/volumes; the dir is just files.
            // Volumes kept (volumes=false) means data-only keep: the compose
            // file is still deletable - a recreate regenerates it - but keep
            // the dir when volumes survive so `docker volume` inspection and
            // any hand-over workflows still have a place to look.
            if volumes {
                let dir = st.instance_dir(&name);
                if dir.exists() {
                    std::fs::remove_dir_all(&dir)
                        .with_context(|| format!("remove {}", dir.display()))?;
                }
            }
            // Total gateway: drop this sandbox's site pair + reload (Phase 2).
            // After the row removal, so the regeneration no longer sees it.
            let line = caddy::regenerate(&st).await?;
            append_log(&log, &format!("{line}\n")).await;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let mut job = shared.lock().await;
        match result {
            Ok(()) => job.status = "ok".into(),
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::error!(sandbox = %name, error = %msg, "delete job failed");
                job.status = "error".into();
                job.error = Some(msg);
            }
        }
        let _ = db::persist_job(&st.db.lock().unwrap(), &job);
    });
    Ok(job_id)
}

/// Spawn a job that deletes one image's tag group (base/app/code-server, S3
/// R3). Async (kind "image-delete", no sandbox) so the images page shows
/// progress via GET /api/jobs/:id; a row is only removed after ALL group
/// rmi succeed (R5: no half-removed row — a mid-group failure keeps the row
/// so a future build upsert re-creates it).
pub async fn spawn_image_delete(state: Arc<AppState>, env_hash: String) -> Result<i64> {
    let job_id = {
        let conn = state.db.lock().unwrap();
        db::insert_job(&conn, "image-delete", None)?
    };
    let shared = Arc::new(TokioMutex::new(JobShared {
        id: job_id,
        kind: "image-delete".into(),
        sandbox: None,
        status: "running".into(),
        error: None,
        log: String::new(),
        log_path: None,
    }));
    {
        let mut jobs = state.jobs.lock().unwrap();
        jobs.insert(job_id, shared.clone());
    }

    let st = state.clone();
    tokio::spawn(async move {
        let log = JobLog::memory(shared.clone());
        let result = run_image_delete(&st, &env_hash, &log).await;
        let mut job = shared.lock().await;
        match result {
            Ok(reclaimed) => {
                job.status = "ok".into();
                job.log
                    .push_str(&format!("image-delete done: reclaimed {reclaimed} bytes\n"));
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::error!(env_hash = %env_hash, error = %msg, "image delete job failed");
                job.status = "error".into();
                job.error = Some(msg);
            }
        }
        let _ = db::persist_job(&st.db.lock().unwrap(), &job);
    });
    Ok(job_id)
}

/// Delete ONE image's tag group. Re-checks refcount (the pre-check in the
/// handler was synchronous; a sandbox may have been created since). Then
/// rmi base -> app -> code-server, skipping missing tags (services off).
/// Returns the total RECLAIMED bytes (sum of each deleted tag's pre-rmi
/// size; missing tags contribute 0).
async fn run_image_delete(st: &Arc<AppState>, env_hash: &str, log: &JobLog) -> Result<u64> {
    {
        let conn = st.db.lock().unwrap();
        let rc = db::image_refcount(&conn, env_hash)?;
        if rc > 0 {
            anyhow::bail!("image is referenced by {rc} sandbox(es); refusing to delete");
        }
    }
    let (base, app, cs) = envhash::image_tags(env_hash);
    let mut reclaimed = 0u64;
    for tag in [&base, &app, &cs] {
        if !docker::image_exists(tag).await? {
            append_log(log, &format!("{tag}: not present, skip\n")).await;
            continue;
        }
        if let Ok(sz) = docker::image_size(tag).await {
            reclaimed += sz;
        }
        match docker::image_rmi(tag).await {
            Ok(out) => append_log(log, &format!("rmi {tag}: {out}")).await,
            Err(e) => {
                anyhow::bail!("rmi {tag} failed (row kept for rebuild): {e:#}");
            }
        }
    }
    let deleted = db::delete_image_row(&st.db.lock().unwrap(), env_hash)?;
    append_log(log, &format!("image row {env_hash} removed ({deleted})\n")).await;
    Ok(reclaimed)
}

/// Spawn the one-shot image cleanup job (S3 R4): delete every refcount=0
/// image group + builder cache, reporting reclaimed space per column.
pub async fn spawn_image_cleanup(state: Arc<AppState>) -> Result<i64> {
    let job_id = {
        let conn = state.db.lock().unwrap();
        db::insert_job(&conn, "image-cleanup", None)?
    };
    let shared = Arc::new(TokioMutex::new(JobShared {
        id: job_id,
        kind: "image-cleanup".into(),
        sandbox: None,
        status: "running".into(),
        error: None,
        log: String::new(),
        log_path: None,
    }));
    {
        let mut jobs = state.jobs.lock().unwrap();
        jobs.insert(job_id, shared.clone());
    }

    let st = state.clone();
    tokio::spawn(async move {
        let log = JobLog::memory(shared.clone());
        let result = run_image_cleanup(&st, &log).await;
        let mut job = shared.lock().await;
        match result {
            Ok(()) => job.status = "ok".into(),
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::error!(error = %msg, "image cleanup job failed");
                job.status = "error".into();
                job.error = Some(msg);
            }
        }
        let _ = db::persist_job(&st.db.lock().unwrap(), &job);
    });
    Ok(job_id)
}

/// Delete every refcount=0 image group + builder cache. Per-row failures are
/// logged and continue (one bad row must not block the rest of the cleanup);
/// the summary line reports images reclaimed (bytes) + cache reclaimed.
async fn run_image_cleanup(st: &Arc<AppState>, log: &JobLog) -> Result<()> {
    let rows = {
        let conn = st.db.lock().unwrap();
        db::list_images(&conn)?
    };
    let mut images_reclaimed: u64 = 0;
    let mut deleted_rows = 0u64;
    let mut failed_rows = 0u64;
    for (env_hash, ..) in rows {
        {
            let conn = st.db.lock().unwrap();
            let rc = db::image_refcount(&conn, &env_hash)?;
            if rc > 0 {
                continue;
            }
        }
        // The delete helper re-checks refcount + removes the row; failures
        // are logged against THIS job and don't abort the rest (R4: one bad
        // row must not block the cleanup).
        match run_image_delete(st, &env_hash, log).await {
            Ok(reclaimed) => {
                deleted_rows += 1;
                images_reclaimed += reclaimed;
            }
            Err(e) => {
                failed_rows += 1;
                append_log(log, &format!("{env_hash}: {e:#}\n")).await;
            }
        }
    }
    // Builder cache prune + summary.
    let prune = match docker::builder_prune().await {
        Ok(out) => out,
        Err(e) => format!("builder prune failed: {e:#}\n"),
    };
    append_log(log, &prune).await;
    append_log(
        log,
        &format!(
            "cleanup done: {deleted_rows} image(s) removed, {failed_rows} failed, \
             reclaimed {images_reclaimed} bytes of images"
        ),
    )
    .await;
    Ok(())
}

/// One message for the create job's single ordered drain task (see the
/// channel comment in run_create).
enum LogMsg {
    /// A chunk of output; docker build's lines arrive newline-terminated.
    Line(String),
    /// Barrier: "everything sent before me has been written". The sender
    /// awaits the ack, so the caller knows both destinations are up to date.
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Non-blocking chunk send. The docker-build callback runs where no `await`
/// is possible, and run_create's own lines must keep the SAME queue (see the
/// ordering note in run_create); a closed channel (runtime shutting down)
/// drops the line rather than failing the job.
fn send_chunk(tx: &mpsc::UnboundedSender<LogMsg>, chunk: impl Into<String>) {
    let _ = tx.send(LogMsg::Line(chunk.into()));
}

/// Wait until every chunk sent so far has reached the memory tail and the
/// on-disk log.
async fn flush_chunks(tx: &mpsc::UnboundedSender<LogMsg>) {
    let (ack, rx) = tokio::sync::oneshot::channel();
    if tx.send(LogMsg::Flush(ack)).is_ok() {
        let _ = rx.await;
    }
}

/// Append-only job log sink (R3). Two destinations, ONE write path:
///   * `cell` - the in-memory tail every job already had (GET /api/jobs/:id,
///     capped at LOG_TAIL);
///   * `file` - the FULL on-disk log (`<data>/jobs/<id>.log`), held open by
///     the create/recreate jobs - the only ones whose output is worth keeping
///     complete (image builds). Short lifecycle jobs keep `None`: their output
///     fits the tail, and the log endpoint falls back to it.
///
/// Both are written under the cell lock so the file's line order can never
/// drift from the tail's.
#[derive(Clone)]
struct JobLog {
    cell: Arc<TokioMutex<JobShared>>,
    file: Option<Arc<StdMutex<std::fs::File>>>,
}

impl JobLog {
    /// Memory-only sink (delete / image-delete / image-cleanup).
    fn memory(cell: Arc<TokioMutex<JobShared>>) -> Self {
        JobLog { cell, file: None }
    }

    /// Memory + full-file sink (create/recreate).
    fn with_file(cell: Arc<TokioMutex<JobShared>>, file: std::fs::File) -> Self {
        JobLog {
            cell,
            file: Some(Arc::new(StdMutex::new(file))),
        }
    }
}

/// Create (truncate) the full-log file of a job. Path: `<data>/jobs/<id>.log`,
/// the same data root as instances/ (state.rs module comment). Job ids are
/// AUTOINCREMENT, so the path is fresh per job; truncate keeps a re-used id
/// (state.db wiped) from appending onto a dead run's output.
fn open_job_log(state: &AppState, job_id: i64) -> Result<(PathBuf, std::fs::File)> {
    let dir = state.data.join("jobs");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(format!("{job_id}.log"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    Ok((path, file))
}

/// Append a chunk to the job log. The in-memory tail is always updated; the
/// full file (when the job opened one) is best-effort - a failed write is
/// warned and the build continues, the disk copy is an observability nicety,
/// never a build dependency.
async fn append_log(log: &JobLog, chunk: &str) {
    let mut job = log.cell.lock().await;
    job.log.push_str(chunk);
    keep_tail(&mut job.log, LOG_TAIL);
    if let Some(file) = &log.file {
        // Blocking write (small, one syscall) with no await while the lock is
        // held - the same "lock briefly, never across an await" rule the
        // std-mutex DB follows.
        match file.lock() {
            Ok(mut f) => {
                if let Err(e) = f.write_all(chunk.as_bytes()) {
                    tracing::warn!(job = job.id, error = %e, "full job log write failed");
                }
            }
            Err(e) => tracing::warn!(job = job.id, error = %e, "full job log lock poisoned"),
        }
    }
}

/// Truncate `s` to its last `cap` bytes, aligned to a char boundary: the cut
/// point can land inside a multibyte char (CJK / emoji build output) and
/// slicing there would panic (R3 fix - the old `append_log` did exactly that,
/// and `tail` below had the correct walk).
fn keep_tail(s: &mut String, cap: usize) {
    if s.len() <= cap {
        return;
    }
    let cut = s.len() - cap;
    let mut start = cut;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    s.drain(..start);
}

/// Public page reader for the R3 log endpoints (routes.rs). Returns
/// `(data, effective_offset, next_offset)`; offsets are BYTES.
///
/// The window is walked onto char boundaries at both ends, so pages
/// concatenate without overlap and without ever splitting a multibyte char
/// (the frontend pages by `next_offset`, not by counting JS string units -
/// UTF-16 length is not a byte offset). A `limit` that lands inside the char
/// at `offset` yields that whole char rather than an empty page the caller
/// would loop on forever.
///
/// `end` is measured from the ALIGNED start: measuring from the raw offset
/// while the start walks forward off a split char can leave `end < start`
/// (small limit, mid-char offset) and panic on the slice - reachable over
/// HTTP via `?offset=<mid-char>&limit=1` / `?tail=<mid-char>`.
pub fn log_page(s: &str, offset: u64, limit: u64) -> (String, u64, u64) {
    let mut start = offset.min(s.len() as u64) as usize;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    let mut end = ((start as u64).saturating_add(limit)).min(s.len() as u64) as usize;
    while end > start && !s.is_char_boundary(end) {
        end -= 1;
    }
    if end == start && start < s.len() {
        end = (start + s[start..].chars().next().map_or(0, char::len_utf8)).min(s.len());
    }
    (s[start..end].to_string(), start as u64, end as u64)
}

/// Byte-tail of a build output, capped like the job log (the images table's
/// build_log column serves the same "tail for the UI" role, design §3.3).
fn tail(s: &str, cap: usize) -> String {
    let mut out = s.to_string();
    keep_tail(&mut out, cap);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_short_input_untouched() {
        assert_eq!(tail("abc", 8), "abc");
        assert_eq!(tail("", 8), "");
    }

    #[test]
    fn tail_long_input_keeps_suffix() {
        let out = format!("{}\nERROR: boom", "x".repeat(LOG_TAIL));
        let t = tail(&out, LOG_TAIL);
        assert!(t.ends_with("ERROR: boom"));
        assert!(t.len() >= LOG_TAIL);
        assert!(!t.starts_with('x') || t.len() == LOG_TAIL);
    }

    #[test]
    fn tail_never_splits_multibyte_chars() {
        // The cut point may land inside a multibyte char; the boundary walk
        // must skip forward, never panic.
        let s = "é".repeat(64); // 2 bytes each, 128 bytes total
        let t = tail(&s, 7); // 7 is odd -> would split an é
        assert!(!t.is_empty());
        assert!(s.ends_with(t.as_str()));
        assert!(!t.contains('\u{FFFD}'), "no replacement char from a split");
    }

    #[test]
    fn append_log_char_boundary_never_panics_on_multibyte() {
        // AC9: the append path (not just `tail`) used to slice at
        // `len - LOG_TAIL` without a boundary walk, so CJK/emoji output
        // straddling the cap panicked ("byte index N is not a char
        // boundary"). keep_tail is the shared fix; this pins it.
        let mut s = "x".repeat(LOG_TAIL + 3) + &"中文🎉".to_string().repeat(4);
        let total = s.len();
        keep_tail(&mut s, LOG_TAIL);
        assert!(s.len() < total, "over-cap input is truncated");
        assert!(s.len() >= LOG_TAIL - 4, "the whole window is kept (mod one char)");
        assert!(!s.contains('\u{FFFD}'), "no half char survives the cut");
        // The kept bytes are a suffix that decodes cleanly.
        assert!(s.chars().all(|c| c == 'x' || "中文🎉".contains(c)));
    }

    #[test]
    fn keep_tail_walks_forward_out_of_a_split_char() {
        let mut s = "é".repeat(64); // 128 bytes, boundaries every 2 bytes
        keep_tail(&mut s, 7); // cut at 121 -> inside an é
        assert_eq!(s.len(), 6, "walked forward to byte 122");
        assert_eq!(s, "ééé");
    }

    #[test]
    fn keep_tail_short_input_untouched() {
        let mut s = "short".to_string();
        keep_tail(&mut s, LOG_TAIL);
        assert_eq!(s, "short");
    }

    /// A `JobLog` on a fresh temp file, plus the shared cell to read back.
    fn temp_job_log(tag: &str) -> (JobLog, Arc<TokioMutex<JobShared>>, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "mgr-r3-jobs-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("1.log");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let cell = Arc::new(TokioMutex::new(JobShared {
            id: 1,
            kind: "create".into(),
            sandbox: Some("sbx-a".into()),
            status: "running".into(),
            error: None,
            log: String::new(),
            log_path: Some(path.display().to_string()),
        }));
        (JobLog::with_file(cell.clone(), file), cell, path)
    }

    #[tokio::test]
    async fn full_log_file_keeps_everything_while_memory_keeps_the_tail() {
        // AC6: the disk log is the WHOLE build output; only the in-memory
        // tail is capped (LOG_TAIL) - that split is what makes the R3 "read
        // the build from the start" endpoints possible.
        let (log, cell, path) = temp_job_log("full");
        let unit = "step\n";
        let lines = LOG_TAIL / unit.len() + 200;
        let mut expected = String::new();
        for i in 0..lines {
            let line = format!("{i} {unit}");
            expected.push_str(&line);
            append_log(&log, &line).await;
        }
        // Flush: File has no user-space buffer, but keep the write visible
        // regardless of platform semantics.
        log.file.as_ref().unwrap().lock().unwrap().flush().unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            on_disk.lines().count(),
            lines,
            "no line is lost on disk (no truncation)"
        );
        assert_eq!(on_disk, expected);

        let in_memory = cell.lock().await.log.clone();
        assert!(
            in_memory.len() <= LOG_TAIL,
            "the in-memory tail stays capped"
        );
        assert!(
            in_memory.len() >= LOG_TAIL - 128,
            "the tail holds roughly the whole window"
        );
        assert!(expected.ends_with(&in_memory), "the tail is a suffix");
    }

    #[tokio::test]
    async fn ordered_drain_flush_is_a_barrier_for_the_file() {
        // AC6: run_create funnels EVERY line (docker build's and its own)
        // through one ordered queue, so a fast producer cannot interleave with
        // - or overtake - the drain task. `flush_chunks` is the barrier that
        // proves the on-disk log is complete WITHOUT closing the channel (the
        // image row pointing at this job's log is written mid-job).
        let (log, _cell, path) = temp_job_log("flush");
        let (tx, mut rx) = mpsc::unbounded_channel::<LogMsg>();
        let drain = log.clone();
        let task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    LogMsg::Line(chunk) => append_log(&drain, &chunk).await,
                    LogMsg::Flush(ack) => {
                        let _ = ack.send(());
                    }
                }
            }
        });
        for i in 0..5000 {
            send_chunk(&tx, format!("build line {i}\n"));
        }
        send_chunk(&tx, "sandbox running\n");
        flush_chunks(&tx).await;
        // Post-barrier the file is complete and in order, without the sender
        // having been dropped.
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk.lines().count(), 5001);
        assert!(on_disk.starts_with("build line 0\n"));
        assert!(on_disk.ends_with("sandbox running\n"));
        drop(tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn memory_only_job_log_leaves_no_file() {
        // Short lifecycle jobs (delete / image-*) keep the pre-R3 shape: no
        // disk file, no path on the cell.
        let (dir, cell) = {
            let dir = std::env::temp_dir().join(format!("mgr-r3-jobs-mem-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let cell = Arc::new(TokioMutex::new(JobShared {
                id: 2,
                kind: "delete".into(),
                sandbox: None,
                status: "running".into(),
                error: None,
                log: String::new(),
                log_path: None,
            }));
            (dir, cell)
        };
        let log = JobLog::memory(cell.clone());
        append_log(&log, "compose down\n").await;
        assert_eq!(cell.lock().await.log, "compose down\n");
        assert!(cell.lock().await.log_path.is_none());
        // Nothing wrote into the directory we created.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn log_page_windows_by_bytes() {
        let s = "abcdefghij"; // 10 bytes
        assert_eq!(log_page(s, 0, 4), ("abcd".into(), 0, 4));
        assert_eq!(log_page(s, 4, 4), ("efgh".into(), 4, 8));
        assert_eq!(log_page(s, 8, 4), ("ij".into(), 8, 10));
        // Offset past EOF clamps to an empty page AT the end (no panic, no
        // negative arithmetic).
        assert_eq!(log_page(s, 99, 4), (String::new(), 10, 10));
        assert_eq!(log_page("", 0, 10), (String::new(), 0, 0));
    }

    #[test]
    fn log_page_never_splits_multibyte_chars() {
        // "中文" = 6 bytes; a 4-byte window would split the second char.
        let s = "中文abc";
        let (data, start, next) = log_page(s, 0, 4);
        assert_eq!(start, 0);
        assert_eq!(data, "中", "the split char is dropped, never emitted half");
        assert_eq!(next, 3, "next_offset is a BYTE offset, not a char count");

        // Resuming from next_offset makes progress even when the limit is
        // smaller than one char: the page still carries the whole char.
        let (data, start, next) = log_page(s, 3, 1);
        assert_eq!((start, data.as_str(), next), (3, "文", 6));

        // ... and never stalls: from 6 onward the tail is plain ASCII.
        let (data, _, next) = log_page(s, 6, 100);
        assert_eq!((data.as_str(), next), ("abc", 9));
    }

    #[test]
    fn log_page_tiny_limit_on_a_boundary_offset_never_panics() {
        // Regression: the window start is walked FORWARD off a split char,
        // while `end` was computed from the RAW offset. With a limit smaller
        // than the remaining bytes of that char, start ended up PAST end and
        // `s[start..end]` panicked ("slice index starts at 3 but ends at 2").
        // Reachable over HTTP: ?tail=<mid-char byte>&limit=1.
        let s = "中文abc";
        let (data, start, next) = log_page(s, 1, 1); // 1 is inside 中
        assert_eq!((start, data.as_str(), next), (3, "文", 6));
        assert_eq!(log_page("中", 1, 1), (String::new(), 3, 3));
        // Every (offset, limit) pair on a multibyte string stays panic-free
        // and never returns bytes a caller would stall on.
        let s = "中🎉abc";
        for off in 0..=s.len() as u64 {
            for lim in 0..=4u64 {
                let (_, start, next) = log_page(s, off, lim);
                assert!(start <= next, "off={off} lim={lim}: {start} > {next}");
                assert!(next > off || next >= s.len() as u64, "no-progress page");
            }
        }
    }

    #[test]
    fn log_page_concatenation_reconstructs_the_log() {
        // The frontend pages by next_offset; joining pages in order must
        // reproduce the original bytes exactly (the paging contract).
        let s = format!("{}\n{}\n{}", "x".repeat(100), "中文🎉".repeat(20), "tail");
        let mut out = String::new();
        let mut off = 0u64;
        loop {
            let (data, start, next) = log_page(&s, off, 7);
            assert_eq!(start, off, "each page starts exactly at the asked offset");
            out.push_str(&data);
            if next == off {
                break;
            }
            off = next;
        }
        assert_eq!(out, s);
    }
}
