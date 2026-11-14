// SPDX-License-Identifier: AGPL-3.0-only
// Capita Money – Android shell (docs/adr/0023-app-shells-and-hosting.md).
// The web app at https://app.capitamoney.com, served from the app itself,
// plus what browsers lack: the chip reader (CapitaChipPlugin) and passkeys
// inside the web view, with the app's identity (Credential Manager).

package com.capitamoney.app;

import android.os.Bundle;
import android.webkit.WebView;

import androidx.webkit.WebSettingsCompat;
import androidx.webkit.WebViewFeature;

import com.getcapacitor.BridgeActivity;

public class MainActivity extends BridgeActivity {
    @Override
    public void onCreate(Bundle savedInstanceState) {
        registerPlugin(CapitaChipPlugin.class);
        super.onCreate(savedInstanceState);
        WebView view = getBridge().getWebView();
        if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_AUTHENTICATION)) {
            WebSettingsCompat.setWebAuthenticationSupport(
                view.getSettings(), WebSettingsCompat.WEB_AUTHENTICATION_SUPPORT_FOR_APP);
        }
    }
}
