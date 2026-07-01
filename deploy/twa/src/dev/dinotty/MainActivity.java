package dev.dinotty.mobile;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.net.Uri;
import android.net.http.SslError;
import android.os.Build;
import android.os.Bundle;
import android.view.KeyEvent;
import android.view.View;
import android.view.Window;
import android.webkit.ClientCertRequest;
import android.webkit.HttpAuthHandler;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.SslErrorHandler;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

/**
 * Dinotty TWA launcher (v0, read-only).
 *
 * <p>Hosts a single WebView pointed at the relay URL passed via the launch intent
 * (e.g. {@code https://<relay>/?desktop_id=<uuid>#<password>}). When no intent
 * data is present (cold launch from the home screen), an offline landing page
 * from {@code file:///android_asset/loading.html} is shown instead.
 *
 * <p>Failures (network / SSL / renderer crash) render an inline dark-themed
 * error page with Retry / Close actions. The {@code intent://} scheme is
 * blocked at the URL-interception layer to mitigate Intent hijacking.
 */
public class MainActivity extends Activity {

    private static final String LOADING_URL = "file:///android_asset/loading.html";
    private static final String BLOCKED_SCHEME = "intent";

    private WebView webView;
    private String launchUrl;
    private boolean errorShown;

    @SuppressLint("SetJavaScriptEnabled")
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        Window window = getWindow();
        window.setBackgroundDrawable(new android.graphics.drawable.ColorDrawable(
                getResources().getColor(android.R.color.black)));

        // Parse intent data once at launch; if absent, fall back to the
        // offline landing screen.
        Uri data = getIntent() != null ? getIntent().getData() : null;
        launchUrl = (data != null && data.toString() != null) ? data.toString() : null;

        // Single root view: a FrameLayout holding the WebView plus an
        // invisible overlay that becomes the error page when needed.
        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(Color.parseColor("#0F172A"));
        setContentView(root);

        webView = new WebView(this);
        webView.setBackgroundColor(Color.parseColor("#0F172A"));
        FrameLayout.LayoutParams lp = new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT);
        root.addView(webView, lp);

        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        // Future-proofing: cookies are unused in v0 but TWAs typically need
        // them once the auth flow lands.
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setLoadWithOverviewMode(true);
        settings.setUseWideViewPort(true);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            settings.setMixedContentMode(WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE);
        }
        CookieManagerBridge.apply(settings);

        // Hide scrollbars visually but keep scroll behaviour.
        webView.setVerticalScrollBarEnabled(false);
        webView.setHorizontalScrollBarEnabled(false);
        webView.setScrollBarStyle(View.SCROLLBARS_OUTSIDE_OVERLAY);

        webView.setWebViewClient(new DinottyWebViewClient());
        webView.setWebChromeClient(new WebChromeClient());

        loadTarget();
    }

    private void loadTarget() {
        String target = (launchUrl != null && !launchUrl.isEmpty()) ? launchUrl : LOADING_URL;
        webView.loadUrl(target);
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (webView != null && launchUrl != null) {
            // Re-entering from the launcher: refresh the relay URL so the user
            // sees a live state instead of a stale cached page.
            webView.loadUrl(launchUrl);
        }
    }

    @Override
    protected void onDestroy() {
        if (webView != null) {
            webView.destroy();
            webView = null;
        }
        super.onDestroy();
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (keyCode == KeyEvent.KEYCODE_BACK && webView != null && webView.canGoBack()) {
            webView.goBack();
            return true;
        }
        return super.onKeyDown(keyCode, event);
    }

    /** Show an inline dark-themed error page overlaying the WebView. */
    private void showErrorPage() {
        if (errorShown) return;
        errorShown = true;

        String html = buildErrorHtml(
                getString(R.string.connection_lost),
                getString(R.string.connection_offline),
                getString(R.string.action_retry),
                getString(R.string.action_close));
        // loadDataWithBaseURL keeps relative resources resolvable and avoids
        // touching the filesystem. baseUrl is null so Android treats it as
        // about:blank for security-context purposes.
        webView.loadDataWithBaseURL(null, html, "text/html", "utf-8", null);
    }

    private void clearErrorState() {
        errorShown = false;
    }

    private String buildErrorHtml(String title, String subtitle,
                                  String retryLabel, String closeLabel) {
        // Inline Material-3 styled error page. Buttons are real <button>s so
        // they're keyboard-focusable and tap-target sized via CSS.
        return "<!DOCTYPE html>\n"
                + "<html><head><meta charset=\"utf-8\"><meta name=\"viewport\""
                + " content=\"width=device-width,initial-scale=1,viewport-fit=cover\">"
                + "<style>\n"
                + "* { box-sizing: border-box; margin: 0; padding: 0; }\n"
                + "html, body { height: 100%; background: #0F172A; color: #E2E8F0;"
                + " font-family: -apple-system, Roboto, sans-serif; }\n"
                + ".wrap { min-height: 100%; display: flex; flex-direction: column;"
                + " align-items: center; justify-content: center; padding: 32px;"
                + " gap: 16px; text-align: center; }\n"
                + ".icon { font-size: 72px; line-height: 1; color: #7C3AED; }\n"
                + "h1 { font-size: 22px; font-weight: 600; color: #E2E8F0; }\n"
                + "p { font-size: 14px; color: #94A3B8; max-width: 320px; }\n"
                + ".actions { display: flex; flex-direction: column; gap: 12px;"
                + " width: 100%; max-width: 320px; margin-top: 16px; }\n"
                + ".btn { display: block; width: 100%; padding: 14px 20px;"
                + " border-radius: 12px; font-size: 15px; font-weight: 600;"
                + " text-align: center; text-decoration: none; border: none;"
                + " cursor: pointer; }\n"
                + ".primary { background: #7C3AED; color: #FFFFFF; }\n"
                + ".secondary { background: #1E293B; color: #E2E8F0; }\n"
                + "</style></head><body>"
                + "<div class=\"wrap\">"
                + "<div class=\"icon\">&#9888;</div>"
                + "<h1>" + escapeHtml(title) + "</h1>"
                + "<p>" + escapeHtml(subtitle) + "</p>"
                + "<div class=\"actions\">"
                + "<button class=\"btn primary\" onclick=\"Dinotty.retry()\">"
                + escapeHtml(retryLabel) + "</button>"
                + "<button class=\"btn secondary\" onclick=\"Dinotty.close()\">"
                + escapeHtml(closeLabel) + "</button>"
                + "</div></div>"
                + "<script>\n"
                + "window.Dinotty = {\n"
                + "  retry: function() { window.location.reload(); },\n"
                + "  close: function() { history.back(); }\n"
                + "};\n"
                + "</script>"
                + "</body></html>";
    }

    private static String escapeHtml(String s) {
        if (s == null) return "";
        StringBuilder out = new StringBuilder(s.length() + 16);
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '<': out.append("&lt;"); break;
                case '>': out.append("&gt;"); break;
                case '&': out.append("&amp;"); break;
                case '"': out.append("&quot;"); break;
                default: out.append(c);
            }
        }
        return out.toString();
    }

    private final class DinottyWebViewClient extends WebViewClient {

        @Override
        public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
            Uri uri = request.getUrl();
            if (uri == null) return false;
            String scheme = uri.getScheme();
            if (scheme != null && scheme.equalsIgnoreCase(BLOCKED_SCHEME)) {
                // PWA intent hijack mitigation: drop intent:// entirely.
                return true;
            }
            // Let everything else (https, http, file://, tel:, mailto:) flow
            // through the WebView normally; the system handles external ones
            // once the user actually taps them.
            return false;
        }

        @Override
        public void onPageStarted(WebView view, String url, Bitmap favicon) {
            super.onPageStarted(view, url, favicon);
            clearErrorState();
        }

        @Override
        public void onReceivedError(WebView view, WebResourceRequest request,
                                    WebResourceResponse errorResponse) {
            // Only the top-level navigation gets the full-screen overlay; sub-
            // resource failures are surfaced through console only.
            if (request != null && request.isForMainFrame()) {
                showErrorPage();
            }
        }

        @Override
        public void onReceivedSslError(WebView view, SslErrorHandler handler,
                                       SslError error) {
            // Refuse and surface the error page; we never silently accept a
            // bad cert in a TWA pointing at a private relay.
            handler.cancel();
            showErrorPage();
        }

        @Override
        public void onReceivedHttpError(WebView view, WebResourceRequest request,
                                        WebResourceResponse errorResponse) {
            if (request != null && request.isForMainFrame()
                    && errorResponse != null
                    && errorResponse.getStatusCode() >= 500) {
                showErrorPage();
            }
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            // The renderer crashed (often OOM). Tear down and surface the
            // error page so the user can retry cleanly.
            if (webView != null) {
                webView.destroy();
            }
            webView = null;
            showErrorPage();
            return true;
        }

        @Override
        public void onReceivedClientCertRequest(WebView view, ClientCertRequest request) {
            // No client-cert auth in v0; cancel so the load fails loudly.
            request.cancel();
        }

        @Override
        public void onReceivedHttpAuthRequest(WebView view, HttpAuthHandler handler,
                                              String host, String realm) {
            handler.cancel();
        }
    }

    /**
     * Thin shim so the no-androidx rule still leaves room to flip cookie
     * handling on later without touching the constructor above.
     */
    private static final class CookieManagerBridge {
        static void apply(WebSettings s) {
            // Cookies are managed via android.webkit.CookieManager at the
            // system level; we don't need to set anything on WebSettings.
            android.webkit.CookieManager cm = android.webkit.CookieManager.getInstance();
            cm.setAcceptCookie(true);
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                cm.setAcceptThirdPartyCookies(null, true);
            }
        }
    }
}