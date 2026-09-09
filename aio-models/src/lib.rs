// aio-models: the canonical model-config store, extracted from the `app`
// crate (sandbox-mgr task Phase 0). `store.rs` is moved here verbatim from
// app/src/routes/models/store.rs — schema + read/write + mask/merge/validate
// with zero axum/http dependency, so both the sandbox app and the mgr
// control plane consume the exact same types and semantics (single owner of
// the canonical models.json contract).

pub mod store;
