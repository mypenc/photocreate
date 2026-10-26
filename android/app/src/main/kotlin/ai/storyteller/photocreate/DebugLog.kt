package ai.storyteller.photocreate

import android.app.Activity
import android.app.ActivityManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Typeface
import android.os.Build
import android.os.Process
import android.util.Log
import android.util.TypedValue
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.concurrent.thread

/**
 * On-device diagnostics for when there is no computer to run `adb logcat`.
 *
 * Launch state machine (a tiny file in `filesDir`, written before the native code starts):
 *   `starting` -> written in [install]; `ready` -> written by [markReady] when Rust has drawn its
 *   first PhotoCreate frame. If the next launch finds `starting`, the previous run died (native
 *   crash, Java crash, killed by the system) BEFORE the first frame. [install] then shows a
 *   non-dismissable dialog with the crash report and three choices: Try loading again (restarts
 *   the whole process), Copy log, Exit.
 *
 * A crash AFTER the first frame is shown once as an ordinary log dialog (Copy / Share / Close),
 * based on a recorded Java crash or the system's exit reason (crash, native crash, ANR).
 *
 * Help > Show Debug Log opens the same report at any time.
 */
object DebugLog {
    private const val MAX_LOG_CHARS = 80_000
    private var dialog: AlertDialog? = null

    private fun stateFile(c: Context) = File(c.filesDir, "launch_state")
    private fun crashFile(c: Context) = File(c.filesDir, "last_crash.txt")
    private fun sessionFlag(c: Context) = File(c.filesDir, "session_active")

    /** Call first thing in `onCreate`, before the native library starts. */
    fun install(activity: Activity) {
        val previousState = runCatching { stateFile(activity).readText().trim() }.getOrDefault("")
        val diedBeforeFirstFrame = previousState == "starting"
        val diedLater = sessionFlag(activity).exists() && !diedBeforeFirstFrame
        runCatching { stateFile(activity).writeText("starting") }
        runCatching { sessionFlag(activity).writeText("1") }

        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching { crashFile(activity).writeText("Thread: ${t.name}\n${Log.getStackTraceString(e)}") }
            previous?.uncaughtException(t, e)
        }

        when {
            diedBeforeFirstFrame -> activity.window.decorView.post { showStartupFailure(activity) }
            diedLater && (crashFile(activity).exists() || lastExitWasProblem(activity)) ->
                activity.window.decorView.postDelayed({ show(activity) }, 1500)
        }
    }

    /** Rust drew its first frame: this launch worked. */
    fun markReady(activity: Activity) {
        runCatching { stateFile(activity).writeText("ready") }
    }

    /** Called from onDestroy when the activity is really finishing. */
    fun markClean(activity: Activity) {
        runCatching { sessionFlag(activity).delete() }
        // Leaving before the first frame (back button on the loading bar) is not a crash.
        runCatching { stateFile(activity).writeText("ready") }
    }

    fun show(activity: Activity) {
        thread {
            val report = buildReport(activity)
            activity.runOnUiThread { present(activity, report, startupFailure = false) }
        }
    }

    private fun showStartupFailure(activity: Activity) {
        thread {
            val report = buildReport(activity)
            activity.runOnUiThread { present(activity, report, startupFailure = true) }
        }
    }

    @Suppress("DEPRECATION")
    private fun lastExitWasProblem(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < 30) return true // no exit reasons before API 30: trust the flag
        return runCatching {
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            val last = am.getHistoricalProcessExitReasons(context.packageName, 0, 1).firstOrNull() ?: return true
            last.reason == android.app.ApplicationExitInfo.REASON_CRASH ||
                last.reason == android.app.ApplicationExitInfo.REASON_CRASH_NATIVE ||
                last.reason == android.app.ApplicationExitInfo.REASON_ANR ||
                last.reason == android.app.ApplicationExitInfo.REASON_INITIALIZATION_FAILURE ||
                last.reason == android.app.ApplicationExitInfo.REASON_SIGNALED
        }.getOrDefault(true)
    }

    private fun exitReasons(context: Context): String {
        if (Build.VERSION.SDK_INT < 30) return "(exit reasons need Android 11+)\n"
        return runCatching {
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            val fmt = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US)
            am.getHistoricalProcessExitReasons(context.packageName, 0, 5).joinToString("") { i ->
                "${fmt.format(Date(i.timestamp))}  reason=${reasonName(i.reason)} status=${i.status} " +
                    "importance=${i.importance} desc=${i.description}\n"
            }.ifEmpty { "(none recorded)\n" }
        }.getOrElse { "(unavailable: $it)\n" }
    }

    private fun reasonName(r: Int): String = when (r) {
        android.app.ApplicationExitInfo.REASON_CRASH -> "JAVA_CRASH"
        android.app.ApplicationExitInfo.REASON_CRASH_NATIVE -> "NATIVE_CRASH"
        android.app.ApplicationExitInfo.REASON_ANR -> "ANR"
        android.app.ApplicationExitInfo.REASON_SIGNALED -> "SIGNALED"
        android.app.ApplicationExitInfo.REASON_LOW_MEMORY -> "LOW_MEMORY"
        android.app.ApplicationExitInfo.REASON_USER_REQUESTED -> "USER_REQUESTED"
        android.app.ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "INIT_FAILURE"
        android.app.ApplicationExitInfo.REASON_EXIT_SELF -> "EXIT_SELF"
        else -> "reason $r"
    }

    private fun buildReport(activity: Activity): String {
        val crash = crashFile(activity)
        val sb = StringBuilder()
        sb.append("PhotoCreate debug report\n")
        sb.append("Device: ${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})\n")
        sb.append("ABIs: ${Build.SUPPORTED_ABIS.joinToString()}\n\n")
        sb.append("--- Previous process exits (system) ---\n").append(exitReasons(activity)).append("\n")
        if (crash.exists()) {
            sb.append("--- Last Java crash ---\n")
            sb.append(runCatching { crash.readText() }.getOrDefault("(unreadable)"))
            sb.append("\n\n")
            runCatching { crash.delete() }
        }
        var logcat = readLogcat()
        if (logcat.length > MAX_LOG_CHARS) logcat = "...(trimmed)\n" + logcat.substring(logcat.length - MAX_LOG_CHARS)
        sb.append("--- logcat (this app) ---\n").append(logcat)
        return sb.toString()
    }

    /** An app may read its own log lines without any permission (the dead process shared our uid). */
    private fun readLogcat(): String = try {
        val process = ProcessBuilder("logcat", "-d", "-v", "time", "-t", "2500").redirectErrorStream(true).start()
        process.inputStream.bufferedReader().readText()
    } catch (e: Exception) {
        "logcat unavailable: $e"
    }

    private fun present(activity: Activity, report: String, startupFailure: Boolean) {
        if (activity.isFinishing || activity.isDestroyed) return
        dialog?.dismiss()
        val pad = (12 * activity.resources.displayMetrics.density).toInt()
        val body = TextView(activity).apply {
            text = report
            setTextIsSelectable(true)
            typeface = Typeface.MONOSPACE
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 10f)
            setPadding(pad, pad, pad, pad)
        }
        val scroll = ScrollView(activity).apply { addView(body) }
        scroll.post { scroll.fullScroll(ScrollView.FOCUS_DOWN) }
        val builder = AlertDialog.Builder(activity).setView(scroll)
        if (startupFailure) {
            // Not dismissable: the user decides. Buttons that must not close the dialog (Copy) are re-wired below.
            builder.setTitle("PhotoCreate closed before it finished loading")
                .setCancelable(false)
                .setPositiveButton("Try loading again", null)
                .setNeutralButton("Copy log", null)
                .setNegativeButton("Exit", null)
        } else {
            builder.setTitle("PhotoCreate log")
                .setPositiveButton("Copy all") { _, _ -> copy(activity, report) }
                .setNeutralButton("Share") { _, _ -> share(activity, report) }
                .setNegativeButton("Close", null)
        }
        val d = builder.create()
        d.show()
        if (startupFailure) {
            d.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener { restartApp(activity) }
            d.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener { copy(activity, report) }
            d.getButton(AlertDialog.BUTTON_NEGATIVE).setOnClickListener { exitApp(activity) }
        }
        dialog = d
    }

    /** Start from scratch: a new process, so the native library loads again. */
    fun restartApp(activity: Activity) {
        runCatching { stateFile(activity).writeText("ready") }
        runCatching { sessionFlag(activity).delete() }
        val launch = activity.packageManager.getLaunchIntentForPackage(activity.packageName)
            ?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
        if (launch != null) {
            // RestartActivity lives in its own process (":restart"), so it survives this process dying.
            val bridge = Intent(activity, RestartActivity::class.java).putExtra(RestartActivity.EXTRA_NEXT, launch)
            activity.startActivity(bridge)
        }
        activity.finishAndRemoveTask()
        Process.killProcess(Process.myPid())
    }

    fun exitApp(activity: Activity) {
        runCatching { stateFile(activity).writeText("ready") }
        runCatching { sessionFlag(activity).delete() }
        activity.finishAndRemoveTask()
        Process.killProcess(Process.myPid())
    }

    private fun copy(context: Context, text: String) {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("PhotoCreate log", text))
        Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
    }

    private fun share(context: Context, text: String) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
        }
        context.startActivity(Intent.createChooser(send, "Share log"))
    }
}
