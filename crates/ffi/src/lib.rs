//! The only unsafe Rust boundary. Callers retain input buffers for the duration of calls.
//! State callbacks run while the Nginx shared-zone mutex is held; inference runs afterward.
use pragma_domain::*;
use pragma_engine::{EngineError, EngineResult, FeatureResult, PartitionState, StateStore};
use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PcSlice {
    pub data: *const u8,
    pub len: usize,
}
#[repr(C)]
pub struct PcHeader {
    pub name: PcSlice,
    pub value: PcSlice,
}
#[repr(C)]
pub struct PcRequest {
    pub timestamp_ms: u64,
    pub method: PcSlice,
    pub path: PcSlice,
    pub host: PcSlice,
    pub query: PcSlice,
    pub client_ip: PcSlice,
    pub body: PcSlice,
    pub headers: *const PcHeader,
    pub header_count: usize,
    pub body_size: u64,
    pub input_error: u32,
}
#[repr(C)]
pub struct PcDecision {
    pub action: u32,
    pub has_score: u32,
    pub score: f64,
    pub reason: [u8; 128],
    pub pipeline_version_id: [u8; 37],
    pub model_version_id: [u8; 37],
}
pub struct Runtime {
    release: Release,
    secret: [u8; 32],
}
pub struct Prepared {
    index: usize,
    features: EngineResult<FeatureResult>,
}
pub type Get = unsafe extern "C" fn(*mut c_void, PcSlice, *mut PcSlice) -> i32;
pub type Put = unsafe extern "C" fn(*mut c_void, PcSlice, PcSlice, usize) -> i32;
struct ForeignStore {
    ctx: *mut c_void,
    get: Get,
    put: Put,
}
unsafe fn bytes<'a>(s: PcSlice) -> EngineResult<&'a [u8]> {
    if s.len == 0 {
        return Ok(&[]);
    }
    if s.data.is_null() || s.len > 64 * 1024 * 1024 {
        return Err(EngineError::Input("invalid FFI buffer".into()));
    }
    Ok(unsafe { slice::from_raw_parts(s.data, s.len) })
}
unsafe fn text(s: PcSlice) -> EngineResult<String> {
    std::str::from_utf8(unsafe { bytes(s) }?)
        .map(str::to_owned)
        .map_err(|_| EngineError::Input("invalid UTF-8".into()))
}
fn borrowed(b: &[u8]) -> PcSlice {
    PcSlice {
        data: b.as_ptr(),
        len: b.len(),
    }
}
impl StateStore for ForeignStore {
    fn get(&mut self, key: &str) -> EngineResult<Option<PartitionState>> {
        let mut out = PcSlice {
            data: ptr::null(),
            len: 0,
        };
        match unsafe { (self.get)(self.ctx, borrowed(key.as_bytes()), &mut out) } {
            0 => Ok(None),
            1 => serde_json::from_slice(unsafe { bytes(out) }?)
                .map(Some)
                .map_err(|e| EngineError::State(e.to_string())),
            _ => Err(EngineError::Capacity),
        }
    }
    fn put(&mut self, key: &str, value: PartitionState, limits: &Limits) -> EngineResult<()> {
        let data = serde_json::to_vec(&value).map_err(|e| EngineError::State(e.to_string()))?;
        if data.len() > limits.max_partition_bytes {
            return Err(EngineError::Capacity);
        }
        if unsafe {
            (self.put)(
                self.ctx,
                borrowed(key.as_bytes()),
                borrowed(&data),
                limits.max_partitions,
            )
        } != 0
        {
            return Err(EngineError::Capacity);
        }
        Ok(())
    }
}
fn copy<const N: usize>(value: &str) -> [u8; N] {
    let mut out = [0; N];
    let bytes = value.as_bytes();
    let n = bytes.len().min(N - 1);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}
fn output(d: Decision) -> PcDecision {
    PcDecision {
        action: match d.action {
            Action::Allow => 0,
            Action::Check => 1,
            Action::Block => 2,
        },
        has_score: u32::from(d.risk_score.is_some()),
        score: d.risk_score.unwrap_or(0.),
        reason: copy(&d.reason),
        pipeline_version_id: copy(&d.pipeline_version_id.0.to_string()),
        model_version_id: copy(
            &d.model_version_id
                .map(|id| id.0.to_string())
                .unwrap_or_default(),
        ),
    }
}
/// # Safety
/// Nonempty slices must point to readable memory; error points to a writable 256-byte buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_runtime_new(
    json: PcSlice,
    secret: PcSlice,
    error: *mut u8,
) -> *mut Runtime {
    let result = catch_unwind(AssertUnwindSafe(|| -> EngineResult<Runtime> {
        let release: Release = serde_json::from_slice(unsafe { bytes(json) }?)
            .map_err(|e| EngineError::Input(e.to_string()))?;
        release
            .validate()
            .map_err(|e| EngineError::Input(e.to_string()))?;
        let s = unsafe { bytes(secret) }?;
        let secret: [u8; 32] = s
            .try_into()
            .map_err(|_| EngineError::Input("32 byte secret required".into()))?;
        Ok(Runtime { release, secret })
    }));
    match result {
        Ok(Ok(runtime)) => Box::into_raw(Box::new(runtime)),
        other => {
            let message = match other {
                Ok(Err(e)) => e.to_string(),
                _ => "runtime initialization panic".into(),
            };
            if !error.is_null() {
                unsafe { ptr::copy_nonoverlapping(copy::<256>(&message).as_ptr(), error, 256) };
            }
            ptr::null_mut()
        }
    }
}
/// # Safety
/// runtime must be a live pointer returned by pc_runtime_new; all slices must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_route(
    runtime: *const Runtime,
    method: PcSlice,
    path: PcSlice,
    host: PcSlice,
    needs_body: *mut u32,
    max_body: *mut usize,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() || needs_body.is_null() || max_body.is_null() {
            return -2;
        }
        let rt = unsafe { &*runtime };
        let (Ok(method), Ok(path), Ok(host)) =
            (unsafe { text(method) }, unsafe { text(path) }, unsafe {
                text(host)
            })
        else {
            return -2;
        };
        for (i, d) in rt.release.deployments.iter().enumerate() {
            let p = &d.pipeline;
            if p.routes().any(|route| route.matches(&host, &method, &path)) {
                unsafe {
                    *needs_body = u32::from(
                        p.needs_body()
                            || p.features
                                .iter()
                                .any(|f| f.operation == Operation::BodySize),
                    );
                    *max_body = p.limits.max_body_bytes;
                }
                return i as i32;
            }
        }
        -1
    }))
    .unwrap_or(-2)
}
unsafe fn event(raw: &PcRequest, p: &Pipeline) -> EngineResult<RequestEvent> {
    let method = unsafe { text(raw.method) }?;
    let path = unsafe { text(raw.path) }?;
    let host = unsafe { text(raw.host) }?;
    let route = p
        .routes()
        .find(|route| route.matches(&host, &method, &path))
        .ok_or_else(|| EngineError::Input("endpoint mismatch".into()))?;
    if raw.input_error != 0 {
        return Err(EngineError::Input(
            "body inspection limit or duplicate input".into(),
        ));
    }
    if raw.header_count > 256 || raw.headers.is_null() && raw.header_count > 0 {
        return Err(EngineError::Input("header limit".into()));
    }
    let mut headers = BTreeMap::new();
    let list = if raw.header_count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(raw.headers, raw.header_count) }
    };
    for h in list {
        let name = unsafe { text(h.name) }?.to_ascii_lowercase();
        // Ignore unrelated headers, including credentials, before constructing the event.
        if !p
            .extractors
            .iter()
            .any(|e| matches!(&e.source,Source::Header{name:n} if n.eq_ignore_ascii_case(&name)))
        {
            continue;
        }
        if headers.insert(name, unsafe { text(h.value) }?).is_some() {
            return Err(EngineError::Input("duplicate selected header".into()));
        }
    }
    let mut query = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(unsafe { bytes(raw.query) }?) {
        if !p
            .extractors
            .iter()
            .any(|e| matches!(&e.source,Source::QueryParameter{name} if *name==key))
        {
            continue;
        }
        if query.insert(key.into_owned(), value.into_owned()).is_some() {
            return Err(EngineError::Input(
                "duplicate selected query parameter".into(),
            ));
        }
    }
    let body = if p.needs_body() {
        Some(
            serde_json::from_slice(unsafe { bytes(raw.body) }?)
                .map_err(|_| EngineError::Input("invalid JSON body".into()))?,
        )
    } else {
        None
    };
    Ok(RequestEvent {
        timestamp_ms: raw.timestamp_ms,
        endpoint_id: route.endpoint_id,
        method,
        path,
        client_ip: unsafe { text(raw.client_ip) }?,
        headers,
        query,
        body,
        body_size: raw.body_size,
    })
}
/// # Safety
/// runtime/request and callbacks must be valid. Caller holds the state-store lock until return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_features(
    runtime: *const Runtime,
    index: i32,
    request: *const PcRequest,
    get: Get,
    put: Put,
    ctx: *mut c_void,
) -> *mut Prepared {
    catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() || request.is_null() || index < 0 {
            return ptr::null_mut();
        }
        let rt = unsafe { &*runtime };
        let Some(d) = rt.release.deployments.get(index as usize) else {
            return ptr::null_mut();
        };
        let f = unsafe { event(&*request, &d.pipeline) }.and_then(|event| {
            pragma_engine::features(
                &d.pipeline,
                &event,
                &rt.secret,
                &mut ForeignStore { ctx, get, put },
            )
        });
        Box::into_raw(Box::new(Prepared {
            index: index as usize,
            features: f,
        }))
    }))
    .unwrap_or(ptr::null_mut())
}
/// # Safety
/// runtime and prepared must be live handles, out must point to writable PcDecision memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_decide(
    runtime: *const Runtime,
    prepared: *const Prepared,
    out: *mut PcDecision,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() || prepared.is_null() || out.is_null() {
            return -1;
        }
        let rt = unsafe { &*runtime };
        let prepared = unsafe { &*prepared };
        let d = &rt.release.deployments[prepared.index];
        let decision = match &prepared.features {
            Ok(f) => pragma_engine::decide(&d.model, d.thresholds, f)
                .unwrap_or_else(|e| Decision::check(d.pipeline.id, e.to_string())),
            Err(e) => Decision::check(d.pipeline.id, e.to_string()),
        };
        unsafe { *out = output(decision) };
        0
    }))
    .unwrap_or(-1)
}
/// # Safety
/// Pass a handle returned by pc_features once, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_prepared_free(value: *mut Prepared) {
    if !value.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe { drop(Box::from_raw(value)) }));
    }
}
/// # Safety
/// Pass a handle returned by pc_runtime_new once, after all uses have completed, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_runtime_free(value: *mut Runtime) {
    if !value.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe { drop(Box::from_raw(value)) }));
    }
}

/// Report the immutable release loaded in this runtime, not the file on disk.
/// # Safety
/// runtime must be live and out must point to 37 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_release_id(runtime: *const Runtime, out: *mut u8) -> i32 {
    if runtime.is_null() || out.is_null() {
        return -1;
    }
    let id = unsafe { &*runtime }.release.release_id.to_string();
    unsafe { ptr::copy_nonoverlapping(copy::<37>(&id).as_ptr(), out, 37) };
    0
}
