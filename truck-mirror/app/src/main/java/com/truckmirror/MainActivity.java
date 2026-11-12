package com.truckmirror;

import android.Manifest;
import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.graphics.Color;
import android.media.projection.MediaProjectionManager;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.util.TypedValue;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

/** The phone screen: two switches to turn on, and whether each one is on. */
public class MainActivity extends Activity implements Mirror.Listener {
    private static final int REQUEST_CAPTURE = 1;

    private TextView captureStatus, touchStatus;
    private Button captureButton;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        int pad = Math.round(20 * getResources().getDisplayMetrics().density);

        LinearLayout col = new LinearLayout(this);
        col.setOrientation(LinearLayout.VERTICAL);
        col.setPadding(pad, pad * 2, pad, pad);

        col.addView(text("Truck Mirror", 28, Color.WHITE));
        col.addView(text("Shows this phone's screen on the truck display through Android Auto.",
                16, 0xFFCBD5E1));

        col.addView(text("\n1. Screen sharing", 20, Color.WHITE));
        captureStatus = text("", 16, 0xFFCBD5E1);
        col.addView(captureStatus);
        captureButton = new Button(this);
        captureButton.setOnClickListener(v -> toggleCapture());
        col.addView(captureButton);
        col.addView(text("When asked, choose to share the entire screen, not a single app. "
                + "Sharing stops if the phone locks, so start it again after unlocking.",
                14, 0xFF94A3B8));

        col.addView(text("\n2. Touch control", 20, Color.WHITE));
        touchStatus = text("", 16, 0xFFCBD5E1);
        col.addView(touchStatus);
        Button touchButton = new Button(this);
        touchButton.setText("Open accessibility settings");
        touchButton.setOnClickListener(v ->
                startActivity(new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)));
        col.addView(touchButton);
        col.addView(text("Turn on “Truck Mirror touch control” there. If the switch is greyed "
                + "out, open this app's App info page, tap the three dots at the top right and "
                + "choose “Allow restricted settings”, then try again.",
                14, 0xFF94A3B8));

        col.addView(text("\n3. On the truck", 20, Color.WHITE));
        col.addView(text("Open Truck Mirror from the Android Auto app list. Netflix and other "
                + "protected apps will show black; that is Android blocking screen capture.",
                16, 0xFFCBD5E1));

        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(0xFF0F172A);
        scroll.addView(col, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        setContentView(scroll);
    }

    @Override
    protected void onResume() {
        super.onResume();
        Mirror.setListener(this);
        refresh();
    }

    @Override
    protected void onPause() {
        // The truck side takes the listener over while it is showing.
        Mirror.setListener(null);
        super.onPause();
    }

    @Override
    public void onMirrorChanged() {
        refresh();
    }

    private void toggleCapture() {
        if (Mirror.isCapturing()) {
            startService(CaptureService.stopIntent(this));
            return;
        }
        if (Build.VERSION.SDK_INT >= 33) {
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 0);
        }
        MediaProjectionManager mpm =
                (MediaProjectionManager) getSystemService(MEDIA_PROJECTION_SERVICE);
        startActivityForResult(mpm.createScreenCaptureIntent(), REQUEST_CAPTURE);
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == REQUEST_CAPTURE && result == RESULT_OK && data != null) {
            startForegroundService(CaptureService.startIntent(this, result, data));
        }
    }

    private void refresh() {
        boolean capturing = Mirror.isCapturing();
        captureStatus.setText(capturing ? "On: your screen is being shared." : "Off.");
        captureButton.setText(capturing ? "Stop screen sharing" : "Start screen sharing");
        touchStatus.setText(touchEnabled()
                ? "On: the truck screen can control this phone."
                : "Off: the truck will show the screen but not respond to touch.");
    }

    private boolean touchEnabled() {
        String enabled = Settings.Secure.getString(getContentResolver(),
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES);
        String mine = new ComponentName(this, TouchService.class).flattenToString();
        return enabled != null && enabled.contains(mine);
    }

    private TextView text(String s, int sp, int color) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextColor(color);
        t.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
        return t;
    }
}
