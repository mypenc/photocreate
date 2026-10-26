package ai.storyteller.cadcraft

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * Runs in its own process (`:restart`, see the manifest) so it outlives the app process that
 * asked for a restart: it launches the real launcher intent and finishes. Same trick as
 * "ProcessPhoenix".
 */
class RestartActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        @Suppress("DEPRECATION")
        val next = intent.getParcelableExtra<Intent>(EXTRA_NEXT)
        if (next != null) startActivity(next)
        finish()
        // Leave this helper process too, so it is not left idle.
        android.os.Process.killProcess(android.os.Process.myPid())
    }

    companion object {
        const val EXTRA_NEXT = "next"
    }
}
