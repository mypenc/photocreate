//! JNI bridge to the Kotlin half (`android/app/src/main/kotlin/…`).
//!
//! Rust → Kotlin: methods on `MainActivity` (`openDialog`, `saveDialog`, `showLog`, `haptic`,
//! clipboard, `localeTag`). Kotlin → Rust: the `Java_ai_storyteller_cadcraft_NativeBridge_*` functions below.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::jlong;
use jni::{JNIEnv, JavaVM};
use cadcraft_ui_egui::Invocation;

static CTX: OnceLock<egui::Context> = OnceLock::new();
static NEXT_ID: AtomicI64 = AtomicI64::new(1);
/// Documents opened from other apps ("Open with"), for the UI thread to open.
static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// What a picker is for. An answer is only handed to a request of the same kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    Open,
    Save,
}

/// The one system picker that is open, and the command that asked for it.
struct Waiting {
    id: i64,
    kind: PickKind,
    asked_by: Option<Invocation>,
}

/// A finished pick waiting for the command that asked to run again and take it.
struct Answer {
    kind: PickKind,
    path: String,
    at: Instant,
}

static WAITING: Mutex<Option<Waiting>> = Mutex::new(None);
static READY: Mutex<Option<Answer>> = Mutex::new(None);
/// The command to run again now that its file is chosen.
static REPLAY: Mutex<Option<Invocation>> = Mutex::new(None);
/// The picker answered for something that was not a command (a dialog button): the user taps again.
static TAP_AGAIN: AtomicBool = AtomicBool::new(false);

/// How long a chosen file stays claimable.
const ANSWER_TTL: Duration = Duration::from_secs(90);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The synchronous `Services::pick_open` / `pick_save` of the desktop UI, made safe for Android.
///
/// The system picker is another activity: while it is up Android destroys our surface and waits
/// for this thread to say it is done, so this thread must NEVER block on the picker (it would
/// deadlock the UI thread against us). Instead: the first call starts the picker and returns
/// `None`; when the user chooses, the host runs the same command again ([`take_replay`]) and this
/// call then returns the chosen path at once.
pub fn pick(app: &AndroidApp, kind: PickKind, name: &str) -> Option<String> {
    {
        let mut ready = lock(&READY);
        if let Some(a) = ready.take_if(|a| a.kind == kind && a.at.elapsed() < ANSWER_TTL) {
            return Some(a.path);
        }
        // A stale or other-kind answer is dropped.
        *ready = None;
    }
    if lock(&WAITING).is_some() {
        return None; // a picker is already open
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    *lock(&WAITING) = Some(Waiting { id, kind, asked_by: cadcraft_ui_egui::current_invocation() });
    let started = with_activity(app, |env, activity| {
        let (method, arg) = match kind {
            PickKind::Open => ("openDialog", "document"),
            PickKind::Save => ("saveDialog", name),
        };
        let arg: JString = env.new_string(arg)?;
        env.call_method(activity, method, "(JLjava/lang/String;)V", &[JValue::Long(id), JValue::Object(&arg)]).map(|_| ())
    });
    if started.is_none() {
        *lock(&WAITING) = None;
        log::warn!("the system file picker could not be started");
    }
    None
}

/// The command that asked for a file which is now chosen: run it again.
pub fn take_replay() -> Option<Invocation> {
    lock(&REPLAY).take()
}

/// A file was chosen for something that cannot be run again (a dialog button): tell the user.
pub fn take_tap_again() -> bool {
    TAP_AGAIN.swap(false, Ordering::Relaxed)
}

pub fn set_context(ctx: egui::Context) {
    let _ = CTX.set(ctx);
}

fn wake() {
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// Documents other apps asked CADCraft to open since the last call.
pub fn take_opened() -> Vec<String> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Run `f` on a thread attached to the JVM with the activity as `this`.
fn with_activity<R>(app: &AndroidApp, f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
    // SAFETY: android-activity guarantees both pointers stay valid for the life of the app.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let mut env = vm.attach_current_thread().ok()?;
    match f(&mut env, &activity) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("JNI call failed: {e}");
            // A pending Java exception would abort the next call.
            let _ = env.exception_clear();
            None
        }
    }
}

/// Help ▸ Show Debug Log: a dialog with this app's log (copy / share).
#[allow(dead_code)]
pub fn show_log(app: &AndroidApp) {
    with_activity(app, |env, activity| env.call_method(activity, "showLog", "()V", &[]).map(|_| ()));
}

/// Help ▸ Full Screen: hide the status and navigation bars and draw into the notch area.
pub fn set_fullscreen(app: &AndroidApp, on: bool) {
    with_activity(app, |env, activity| env.call_method(activity, "setFullscreen", "(Z)V", &[JValue::Bool(u8::from(on))]).map(|_| ()));
}

/// Whether a documents folder is chosen and still accessible.
pub fn documents_folder_set(app: &AndroidApp) -> Option<bool> {
    with_activity(app, |env, activity| env.call_method(activity, "documentsFolderSet", "()Z", &[])?.z())
}

/// Help ▸ Documents Folder: `true` opens the folder picker, `false` forgets the folder.
pub fn set_documents_folder(app: &AndroidApp, on: bool) {
    with_activity(app, |env, activity| env.call_method(activity, "setDocumentsFolder", "(Z)V", &[JValue::Bool(u8::from(on))]).map(|_| ()));
}

/// A short vibration (the long press that opens a context menu).
pub fn haptic(app: &AndroidApp) {
    with_activity(app, |env, activity| env.call_method(activity, "haptic", "()V", &[]).map(|_| ()));
}

/// Whether Android currently reports the on-screen keyboard as visible.
pub fn soft_keyboard_visible(app: &AndroidApp) -> Option<bool> {
    with_activity(app, |env, activity| env.call_method(activity, "softKeyboardVisible", "()Z", &[])?.z())
}

/// Hide the on-screen keyboard without dropping focus from the document.
pub fn hide_soft_keyboard(app: &AndroidApp) {
    with_activity(app, |env, activity| env.call_method(activity, "hideSoftKeyboard", "()V", &[]).map(|_| ()));
}

/// Open the on-screen keyboard by force (the bar's KB button), even where the usual request is ignored.
pub fn force_show_soft_keyboard(app: &AndroidApp) {
    with_activity(app, |env, activity| env.call_method(activity, "forceShowSoftKeyboard", "()V", &[]).map(|_| ()));
}

/// Report real startup progress (0 to 100) to the loading bar.
pub fn set_loading_progress(app: &AndroidApp, percent: i32) {
    with_activity(app, |env, activity| env.call_method(activity, "setLoadingProgress", "(I)V", &[JValue::Int(percent)]).map(|_| ()));
}

/// Remove the startup loading bar once CADCraft's first frame is on screen.
pub fn hide_loading(app: &AndroidApp) {
    with_activity(app, |env, activity| env.call_method(activity, "hideLoading", "()V", &[]).map(|_| ()));
}

/// The system clipboard's text.
pub fn clipboard_get(app: &AndroidApp) -> Option<String> {
    with_activity(app, |env, activity| {
        let value = env.call_method(activity, "clipboardText", "()Ljava/lang/String;", &[])?.l()?;
        if value.as_raw().is_null() {
            return Ok(None);
        }
        let value = JString::from(value);
        let text = env.get_string(&value)?;
        Ok(Some(String::from(text)))
    })
    .flatten()
}

/// Put `text` on the system clipboard.
pub fn clipboard_set(app: &AndroidApp, text: &str) {
    with_activity(app, |env, activity| {
        let text: JString = env.new_string(text)?;
        env.call_method(activity, "setClipboardText", "(Ljava/lang/String;)V", &[JValue::Object(&text)]).map(|_| ())
    });
}

fn read_string(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.as_raw().is_null() {
        return None;
    }
    env.get_string(s).ok().map(String::from)
}

/// `NativeBridge.onDialogAnswer(id: Long, path: String?)`, from a Kotlin worker or the UI thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_cadcraft_NativeBridge_onDialogAnswer(mut env: JNIEnv, _class: JClass, id: jlong, path: JString) {
    let answer = read_string(&mut env, &path);
    let waiting = {
        let mut w = lock(&WAITING);
        if w.as_ref().is_some_and(|w| w.id == id) { w.take() } else { None }
    };
    if let (Some(w), Some(path)) = (waiting, answer) {
        *lock(&READY) = Some(Answer { kind: w.kind, path, at: Instant::now() });
        match w.asked_by {
            Some(inv) => *lock(&REPLAY) = Some(inv),
            None => TAP_AGAIN.store(true, Ordering::Relaxed),
        }
    }
    wake();
}

/// `NativeBridge.onOpenIntent(path: String)`: a document opened from another app.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_cadcraft_NativeBridge_onOpenIntent(mut env: JNIEnv, _class: JClass, path: JString) {
    if let Some(p) = read_string(&mut env, &path) {
        OPENED.lock().unwrap_or_else(PoisonError::into_inner).push(p);
        wake();
    }
}

/// The device's language as a BCP 47 tag ("en-US"), for the interface language.
pub fn locale_tag(app: &AndroidApp) -> Option<String> {
    with_activity(app, |env, activity| {
        let value = env.call_method(activity, "localeTag", "()Ljava/lang/String;", &[])?.l()?;
        if value.as_raw().is_null() {
            return Ok(None);
        }
        let value = JString::from(value);
        let text = env.get_string(&value)?;
        Ok(Some(String::from(text)))
    })
    .flatten()
}
