package {{reverse-domain app.domain}}.{{snake-case app.name}}

import android.webkit.*
import android.annotation.*

class RustWebViewClient: WebViewClient() {
    override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest?): Boolean {
        return false
    }

    override fun shouldInterceptRequest(
        view: WebView,
        request: WebResourceRequest
    ): WebResourceResponse? {
        return handleRequest(request)
    }

    @SuppressLint("WebViewClientOnReceivedSslError")
    override fun onReceivedSslError(view: WebView?, handler: SslErrorHandler, error: android.net.http.SslError) {
      if (allowSslError(error.url)) {
        handler.proceed()
      } else {
        handler.cancel()
      }
    }

    companion object {
        init {
            System.loadLibrary("{{snake-case app.name}}")
        }
    }

    private external fun allowSslError(url: String): Boolean
    private external fun handleRequest(request: WebResourceRequest): WebResourceResponse?
}