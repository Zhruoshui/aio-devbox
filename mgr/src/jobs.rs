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

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Mutex as TokioMutex;

use crate::caddy;
use crate::composegen;
use crate::db;
use crate::docker;
use crate::envhash;
use crate::state::{AppState, JobShared};

/// Log tail cap for the job record. The progress view shows the tail; the
/// full output is not persisted (personal scale).
const LOG_TAIL: usize = 8 * 1024;

/// Spawn the create/recreate job for a sandbox. All parameters are owned -
/// the task outlives the request. Returns the job id.
pub async fn spawn_create(
    state: Arc<AppState>,
    name: String,
    env: envhash::SandboxEnv,
    cpus: Option<f64>,
    mem_mb: Option<i64>,
    recreate: bool,
) -> Result<i64> {
    // Durable job row first so a crash between spawn and first update still
    // leaves a traceable job.
    let job_id = {
        let conn = state.db.lock().unwrap();
        db::insert_job(&conn, if recreate { "recreate" } else { "create" }, Some(&name))?
    };

    let shared = Arc::new(TokioMutex::new(JobShared {
        id: job_id,
        kind: if recreate { "recreate" } else { "create" }.into(),
        sandbox: Some(name.clone()),
        status: "running".into(),
        error: None,
        log: String::new(),
    }));
    {
        let mut jobs = state.jobs.lock().unwrap();
        jobs.insert(job_id, shared.clone());
    }

    let st = state.clone();
    tokio::spawn(async move {
        let result = run_create(st.clone(), name.clone(), env, cpus, mem_mb, recreate, shared.clone()).await;
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

async fn run_create(
    state: Arc<AppState>,
    name: String,
    env: envhash::SandboxEnv,
    cpus: Option<f64>,
    mem_mb: Option<i64>,
    recreate: bool,
    log: Arc<TokioMutex<JobShared>>,
) -> Result<()> {
    let repo = state.repo.clone();
    let instance = state.instance_dir(&name);
    std::fs::create_dir_all(&instance)
        .with_context(|| format!("create {}", instance.display()))?;

    // 1. env -> manifest -> assembled Dockerfile.base content -> env_hash.
    //    Validation happens here (not just in the API) so a job replay from
    //    a stale DB still fails safely.
    let manifest = env.to_manifest_checked(&repo)?;
    let (dockerfile_base, _display) =
        aio_config::gen::assemble_for(&repo, &manifest)?;
    let hash = envhash::env_hash(&dockerfile_base);
    let (base_tag, app_tag, cs_tag) = envhash::image_tags(&hash);

    append_log(&log, &format!(
        "env_hash {hash}\nbase assembly ok, images: {base_tag} / {app_tag} / {cs_tag}\n"
    )).await;

    // 2. Shared network (idempotent) - required by the generated compose.
    docker::ensure_network("aio-mgr-net").await?;
    append_log(&log, "network aio-mgr-net ok\n").await;

    // 3. Images: build missing ones. Order base -> app -> code-server -> vnc
    //    (dependency: app/cs FROM base). Same-env reuse (A5) hits the
    //    existence checks and skips everything.
    if !docker::image_exists(&base_tag).await? {
        append_log(&log, &format!("building {base_tag} ...\n")).await;
        // Per-sandbox Dockerfile.base so builds don't share the repo-root
        // output (D2: everything a sandbox needs lives in its instance dir).
        let df_path = instance.join("Dockerfile.base");
        std::fs::write(&df_path, &dockerfile_base)
            .with_context(|| format!("write {}", df_path.display()))?;
        let out = docker::build(&repo, &df_path, &base_tag, &[]).await?;
        append_log(&log, &out).await;
    } else {
        append_log(&log, &format!("{base_tag} exists, skip\n")).await;
    }
    for (tag, dockerfile) in [
        (&app_tag, "app/Dockerfile"),
        (&cs_tag, "code-server/Dockerfile"),
    ] {
        if !docker::image_exists(tag).await? {
            append_log(&log, &format!("building {tag} ...\n")).await;
            let out = docker::build(&repo, &repo.join(dockerfile), tag, &[("BASE_IMAGE", &base_tag)])
                .await?;
            append_log(&log, &out).await;
        } else {
            append_log(&log, &format!("{tag} exists, skip\n")).await;
        }
    }
    if !docker::image_exists(envhash::VNC_TAG).await? {
        append_log(&log, &format!("building {} ...\n", envhash::VNC_TAG)).await;
        let out = docker::build(&repo, &repo.join("vnc/Dockerfile"), envhash::VNC_TAG, &[]).await?;
        append_log(&log, &out).await;
    }

    {
        let conn = state.db.lock().unwrap();
        db::upsert_image(&conn, &hash, &base_tag, "").with_context(|| "record image")?;
    }

    // 4. Compose + Caddyfile.
    let gen = composegen::generate(&name, &hash, cpus, mem_mb)?;
    composegen::write(&instance, &gen)?;
    append_log(&log, "compose.yml + gateway/Caddyfile written\n").await;

    // 5. up -d (force-recreate on env change keeps volumes - design §3.6).
    let project = envhash::project_name(&name);
    let compose_file = instance.join("compose.yml");
    let out = docker::compose_up(&project, &compose_file, recreate).await?;
    append_log(&log, &out).await;

    // 6. Persist the env/config on success (A5 correctness: a failed create
    //    doesn't overwrite a working sandbox's config).
    {
        let conn = state.db.lock().unwrap();
        if recreate {
            db::update_sandbox_config(&conn, &name, &env.canonical_json(), &hash, cpus, mem_mb)?;
        } else {
            db::update_sandbox_config(&conn, &name, &env.canonical_json(), &hash, cpus, mem_mb)?;
        }
    }

    // 7. Total gateway: regenerate the Caddyfile with this sandbox's site
    //    pair + reload (Phase 2). Reported into the job log on both paths;
    //    a reload failure keeps the .bak and does not fail the job.
    match caddy::regenerate(&state).await {
        Ok(line) => append_log(&log, &format!("{line}\n")).await,
        Err(e) => append_log(&log, &format!("gateway regenerate FAILED: {e:#}\n")).await,
    }

    append_log(&log, "sandbox running\n").await;
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
    }));
    {
        let mut jobs = state.jobs.lock().unwrap();
        jobs.insert(job_id, shared.clone());
    }

    let st = state.clone();
    tokio::spawn(async move {
        let result = async {
            let compose_file = st.instance_dir(&name).join("compose.yml");
            let project = envhash::project_name(&name);
            let out = docker::compose_down(&project, &compose_file, volumes).await?;
            append_log(&shared, &out).await;
            {
                let conn = st.db.lock().unwrap();
                db::delete_sandbox(&conn, &name)?;
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
            append_log(&shared, &format!("{line}\n")).await;
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

async fn append_log(log: &Arc<TokioMutex<JobShared>>, chunk: &str) {
    let mut job = log.lock().await;
    job.log.push_str(chunk);
    if job.log.len() > LOG_TAIL {
        let cut = job.log.len() - LOG_TAIL;
        job.log = job.log[cut..].to_string();
    }
}
