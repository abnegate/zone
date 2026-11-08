package com.abnegate.zone

import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import java.util.Locale

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var pendingWebViewState: Bundle? = null
  private var topCss = "0px"
  private var rightCss = "0px"
  private var bottomCss = "0px"
  private var leftCss = "0px"

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    pendingWebViewState = savedInstanceState
    super.onCreate(savedInstanceState)
    ViewCompat.setOnApplyWindowInsetsListener(window.decorView) { _, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val density = resources.displayMetrics.density
      topCss = cssPx(bars.top, density)
      rightCss = cssPx(bars.right, density)
      bottomCss = cssPx(bars.bottom, density)
      leftCss = cssPx(bars.left, density)
      applySafeArea()
      insets
    }
  }

  override fun onPause() {
    persistCurrentPath()
    super.onPause()
    webView?.onResume()
  }

  override fun onSaveInstanceState(outState: Bundle) {
    super.onSaveInstanceState(outState)
    persistCurrentPath()
    webView?.saveState(outState)
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      webView.setRendererPriorityPolicy(WebView.RENDERER_PRIORITY_BOUND, false)
    }
    val state = pendingWebViewState
    pendingWebViewState = null
    val href = restoreHref()
    webView.post {
      var restored = false
      if (state != null) {
        webView.stopLoading()
        restored = webView.restoreState(state) != null
      }
      if (!restored && href != null && isLoopbackRoot(webView.url)) {
        webView.stopLoading()
        webView.loadUrl(href)
      }
      applySafeArea()
    }
    ViewCompat.requestApplyInsets(window.decorView)
    applySafeArea()
    for (delay in longArrayOf(0L, 200L, 500L, 1000L, 2000L)) {
      webView.postDelayed({ applySafeArea() }, delay)
    }
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (hasFocus) {
      applySafeArea()
    }
  }

  private fun persistCurrentPath() {
    val view = webView ?: return
    view.evaluateJavascript(
      """
      (function(){
        try {
          if (typeof window.__zonePersistCurrentPath === 'function') {
            window.__zonePersistCurrentPath();
          }
          return window.location.href;
        } catch (e) {
          return null;
        }
      })();
      """.trimIndent()
    ) { value ->
      rememberHref(unquoteJsString(value))
    }
  }

  private fun rememberHref(href: String?) {
    if (href.isNullOrBlank()) {
      return
    }
    if (!href.startsWith(LOOPBACK)) {
      return
    }
    val path = pathOf(href)
    if (AUTH_PREFIXES.any { path == it || path.startsWith("$it/") }) {
      return
    }
    getSharedPreferences(PREFS, MODE_PRIVATE).edit().putString(LAST_HREF, href).commit()
  }

  private fun restoreHref(): String? {
    val href = getSharedPreferences(PREFS, MODE_PRIVATE).getString(LAST_HREF, null)
    if (href.isNullOrBlank() || !href.startsWith(LOOPBACK)) {
      return null
    }
    val rest = href.removePrefix(LOOPBACK)
    if (rest.isEmpty() || rest == "/") {
      return null
    }
    val path = pathOf(href)
    if (AUTH_PREFIXES.any { path == it || path.startsWith("$it/") }) {
      return null
    }
    return href
  }

  private fun applySafeArea() {
    val view = webView ?: return
    val script =
      """
      (function(){
        var root = document.documentElement;
        if (!root) return;
        root.style.setProperty('--ui-safe-top', '$topCss');
        root.style.setProperty('--ui-safe-right', '$rightCss');
        root.style.setProperty('--ui-safe-bottom', '$bottomCss');
        root.style.setProperty('--ui-safe-left', '$leftCss');
        root.style.setProperty('--safe-top', '$topCss');
        root.style.setProperty('--safe-right', '$rightCss');
        root.style.setProperty('--safe-bottom', '$bottomCss');
        root.style.setProperty('--safe-left', '$leftCss');
      })();
      """.trimIndent()
    view.post { view.evaluateJavascript(script, null) }
  }

  private fun cssPx(pixels: Int, density: Float): String {
    return String.format(Locale.US, "%.2fpx", pixels / density)
  }

  companion object {
    private const val PREFS = "zone"
    private const val LAST_HREF = "last_href"
    private const val LOOPBACK = "http://127.0.0.1:24727"
    private val AUTH_PREFIXES = arrayOf(
      "/login",
      "/register",
      "/verify-email",
      "/verify",
      "/forgot-password",
      "/reset-password",
      "/invitations",
      "/unauthorized",
      "/agent-sign-in",
    )

    private fun pathOf(href: String): String {
      val rest = href.removePrefix(LOOPBACK)
      val path = if (rest.startsWith("/")) rest else href
      return path.split('?', '#')[0].ifEmpty { "/" }
    }

    private fun isLoopbackRoot(url: String?): Boolean {
      if (url.isNullOrBlank() || url == "about:blank") {
        return true
      }
      return url == LOOPBACK || url == "$LOOPBACK/"
    }

    private fun unquoteJsString(value: String?): String? {
      if (value.isNullOrBlank() || value == "null") {
        return null
      }
      return value.removeSurrounding("\"")
    }
  }
}
